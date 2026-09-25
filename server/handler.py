"""Сервер Echips (Yandex Cloud Functions, Python): вход по PIN, приём отчётов, профили, инженеры, журнал событий.
Секретов в коде нет: SESSION_SECRET и BUCKET задаются переменными окружения функции.
Формат запроса — API Gateway (payload 1.0): httpMethod, path, headers, queryStringParameters, body, isBase64Encoded."""
import base64
import hashlib
import hmac
import json
import os
import re
import secrets
import time

from storage import S3Storage

try:
    from ecdsa import NIST256p, SigningKey
    from ecdsa.util import sigencode_string
except ImportError:      # без библиотеки аренды не выдаём (клиент просто не получит lease)
    SigningKey = None

MSK = 3 * 3600
LEASE_TTL = 7 * 86400
LEASE_ITERS = 200_000

SESSION_TTL = 12 * 3600
MAX_FAILS = 5
FAIL_WINDOW = 15 * 60
PIN_RE = re.compile(r"^\d{4,12}$")
ID_RE = re.compile(r"^[a-z0-9_-]{2,32}$", re.I)
SEG_RE = re.compile(r"^[\w.\-]{1,60}$", re.U)   # буквы (в т.ч. кириллица), цифры, . _ -


class HttpError(Exception):
    def __init__(self, code, msg):
        self.code, self.msg = code, msg


def now():
    return int(time.time())


def resp(code, obj=None, raw=None, ctype="application/json; charset=utf-8"):
    body = raw if raw is not None else json.dumps(obj if obj is not None else {}, ensure_ascii=False)
    return {"statusCode": code, "headers": {"Content-Type": ctype, "Cache-Control": "no-store"}, "body": body, "isBase64Encoded": False}


# ---------- сессии ----------
def _secret():
    s = os.environ.get("SESSION_SECRET", "")
    if len(s) < 32:
        raise HttpError(500, "SESSION_SECRET не задан")
    return s.encode()


def make_token(user, machine_id, ttl=SESSION_TTL):
    payload = {"sub": user["id"], "name": user["name"], "role": user["role"], "mid": machine_id, "exp": now() + ttl}
    b = base64.urlsafe_b64encode(json.dumps(payload, ensure_ascii=False).encode()).decode().rstrip("=")
    sig = hmac.new(_secret(), b.encode(), hashlib.sha256).hexdigest()
    return b + "." + sig, payload


def read_token(tok):
    try:
        b, sig = tok.split(".", 1)
        if not hmac.compare_digest(sig, hmac.new(_secret(), b.encode(), hashlib.sha256).hexdigest()):
            return None
        payload = json.loads(base64.urlsafe_b64decode(b + "=" * (-len(b) % 4)))
        return payload if payload.get("exp", 0) > now() else None
    except Exception:
        return None


# ---------- инженеры ----------
def load_users(store):
    raw = store.get("config/users.json")
    return json.loads(raw) if raw else []


def save_users(store, users):
    store.put("config/users.json", json.dumps(users, ensure_ascii=False, indent=2).encode())


def scrypt_hash(pin, salt):
    return hashlib.scrypt(pin.encode(), salt=bytes.fromhex(salt), n=2 ** 14, r=8, p=1, dklen=32).hex()


def make_user(uid, name, pin, role):
    salt = secrets.token_hex(16)
    return {"id": uid, "name": name, "role": "admin" if role == "admin" else "tech", "scheme": "scrypt", "salt": salt, "pin_hash": scrypt_hash(pin, salt), "active": True}


def check_pin(user, pin):
    if not user.get("active", True):
        return False
    if user.get("scheme") == "scrypt":
        return hmac.compare_digest(scrypt_hash(pin, user["salt"]), user["pin_hash"])
    # исходная схема (перенос из приложения): sha256(salt:pin)
    return hmac.compare_digest(hashlib.sha256((user["salt"] + ":" + pin).encode()).hexdigest(), user["pin_hash"])


# ---------- аренда для входа без интернета ----------
def make_lease(user, pin, machine_id):
    """Подписанная аренда на 7 суток: клиент офлайн сверяет PIN с проверочным значением (PBKDF2), а подпись
    (ECDSA P-256, ключ LEASE_KEY, публичная часть вшита в exe) не даёт подделать или продлить файл."""
    key = os.environ.get("LEASE_KEY", "")
    if not key or SigningKey is None:
        return None
    salt = secrets.token_hex(16)
    verifier = hashlib.pbkdf2_hmac("sha256", pin.encode(), bytes.fromhex(salt), LEASE_ITERS).hex()
    t = now()
    payload = json.dumps({"sub": user["id"], "name": user["name"], "role": user["role"], "mid": machine_id, "iat": t,
                          "exp": t + LEASE_TTL, "salt": salt, "iters": LEASE_ITERS, "verifier": verifier},
                         ensure_ascii=False, separators=(",", ":"))
    sk = SigningKey.from_string(bytes.fromhex(key), curve=NIST256p)
    sig = sk.sign(payload.encode(), hashfunc=hashlib.sha256, sigencode=sigencode_string).hex()
    return {"payload": payload, "sig": sig}


# ---------- журнал ----------
def log_event(store, typ, who, ip, data=None):
    t = time.time()
    day = time.strftime("%Y-%m-%d", time.gmtime(t + MSK))   # дни журнала — по московскому времени
    ev = {"t": int(t), "type": typ, "user": who, "ip": ip, "data": data or {}}
    store.put("events/%s/%d-%s.json" % (day, int(t * 1000), secrets.token_hex(3)), json.dumps(ev, ensure_ascii=False).encode())


# ---------- вспомогательное ----------
def hdr(event, name):
    for k, v in (event.get("headers") or {}).items():
        if k.lower() == name.lower():
            return v
    return ""


def client_ip(event):
    ip = ((event.get("requestContext") or {}).get("identity") or {}).get("sourceIp", "")
    return ip or hdr(event, "X-Forwarded-For").split(",")[0].strip()


def body_json(event):
    b = event.get("body") or ""
    if event.get("isBase64Encoded"):
        b = base64.b64decode(b).decode()
    try:
        return json.loads(b) if b else {}
    except ValueError:
        raise HttpError(400, "Некорректный JSON")


def auth(event, admin=False):
    h = hdr(event, "Authorization")
    payload = read_token(h[7:]) if h.lower().startswith("bearer ") else None
    if not payload:
        raise HttpError(401, "Сессия недействительна — войдите заново")
    if admin and payload["role"] != "admin":
        raise HttpError(403, "Только для администратора")
    return payload


def safe_path(rel, engineer_name):
    """Путь отчёта: <инженер>/<дата>/<устройство>/<файл> (или старый <инженер>/<дата>/<файл>).
    Первый сегмент принудительно заменяется на имя из сессии — инженер не может писать под чужим именем."""
    parts = [p for p in rel.split("/") if p != ""]
    if len(parts) not in (3, 4) or any(p in (".", "..") or not SEG_RE.match(p) for p in parts):
        raise HttpError(400, "Некорректный путь отчёта")
    if not re.match(r"^\d{4}-\d{2}-\d{2}$", parts[1]):
        raise HttpError(400, "Некорректная дата в пути отчёта")
    parts[0] = re.sub(r"[^\w.\-]", "_", engineer_name, flags=re.U)[:30] or "unknown"
    return "/".join(parts)


# ---------- маршруты ----------
def route_login(store, event):
    ip = client_ip(event)
    b = body_json(event)
    pin = str(b.get("pin", ""))
    rl_key = "rl/" + hashlib.sha256(ip.encode()).hexdigest()[:16] + ".json"
    raw = store.get(rl_key)
    fails = [t for t in (json.loads(raw) if raw else []) if t > now() - FAIL_WINDOW]
    if len(fails) >= MAX_FAILS:
        log_event(store, "login_blocked", None, ip, {"machine": b.get("machine_id")})
        raise HttpError(429, "Слишком много неверных попыток. Подождите 15 минут.")
    users = load_users(store)
    found = None
    if PIN_RE.match(pin):
        for u in users:
            if check_pin(u, pin):
                found = u
                break
    if not found:
        fails.append(now())
        store.put(rl_key, json.dumps(fails).encode())
        log_event(store, "login_fail", None, ip, {"machine": b.get("machine_id"), "name": b.get("machine_name")})
        raise HttpError(401, "Неверный PIN")
    if found.get("scheme") != "scrypt":       # первый успешный вход — переводим на scrypt
        for i, u in enumerate(users):
            if u["id"] == found["id"]:
                users[i] = dict(make_user(u["id"], u["name"], pin, u["role"]))
        save_users(store, users)
    store.delete(rl_key)
    mid = str(b.get("machine_id", ""))[:64]
    tok, payload = make_token(found, mid)
    log_event(store, "login", found["id"], ip, {"machine": mid, "name": str(b.get("machine_name", ""))[:64], "app": str(b.get("app_version", ""))[:16]})
    out = {"token": tok, "exp": payload["exp"], "user": {"id": found["id"], "name": found["name"], "role": found["role"]}}
    lease = make_lease(found, pin, mid)
    if lease:
        out["lease"] = lease
    return resp(200, out)


def route_report(store, event):
    who = auth(event)
    b = body_json(event)
    path = safe_path(str(b.get("path", "")), who["name"])
    report_json = b.get("json")
    if not isinstance(report_json, str) or len(report_json) > 3_000_000:
        raise HttpError(400, "Нет тела отчёта")
    json.loads(report_json)
    store.put("reports/%s.json" % path, report_json.encode())
    pdf = b.get("pdf_b64")
    if pdf:
        store.put("reports/%s.pdf" % path, base64.b64decode(pdf))
    log_event(store, "report", who["sub"], client_ip(event), {"path": path, "size": len(report_json), "machine": who.get("mid"), "kind": str(b.get("kind", ""))[:16]})
    return resp(200, {"ok": True, "path": path})


def route_reports_list(store, event):
    auth(event, admin=True)
    out = []
    for key in store.list("reports/"):
        if not key.endswith(".json"):
            continue
        parts = key[len("reports/"):].split("/")
        if len(parts) == 4:
            eng, date, dev, f = parts
        elif len(parts) == 3:
            eng, date, f = parts
            dev = f[:-5].split("_", 1)[-1]
        else:
            continue
        out.append({"path": "/".join(parts), "engineer": eng, "date": date, "device": dev, "file": f})
    out.sort(key=lambda r: (r["date"], r["file"]), reverse=True)
    return resp(200, out)


def route_report_get(store, event):
    auth(event, admin=True)
    path = ((event.get("queryStringParameters") or {}).get("path") or "")
    if ".." in path or not path.endswith(".json"):
        raise HttpError(400, "Некорректный путь отчёта")
    raw = store.get("reports/" + path)
    if raw is None:
        raise HttpError(404, "Отчёт не найден")
    return resp(200, raw=raw.decode())


def route_profiles_get(store, event):
    auth(event)
    raw = store.get("config/profiles.json")
    return resp(200, raw=raw.decode() if raw else json.dumps({"models": {}}))


def route_profiles_put(store, event):
    who = auth(event, admin=True)
    b = body_json(event)
    key, prof = str(b.get("key", "")).strip(), b.get("profile")
    if not key or not isinstance(prof, dict):
        raise HttpError(400, "Нужны key и profile")
    raw = store.get("config/profiles.json")
    cur = json.loads(raw) if raw else {"models": {}}
    cur.setdefault("models", {})[key] = prof
    cur["version"] = cur.get("version", 0) + 1
    store.put("config/profiles.json", json.dumps(cur, ensure_ascii=False, indent=2).encode())
    log_event(store, "profile_save", who["sub"], client_ip(event), {"key": key})
    return resp(200, cur)


def public_users(users):
    return [{"id": u["id"], "name": u["name"], "role": u["role"], "active": u.get("active", True)} for u in users]


def route_users(store, event, method):
    who = auth(event, admin=True)
    users = load_users(store)
    ip = client_ip(event)
    if method == "GET":
        return resp(200, public_users(users))
    if method == "POST":
        b = body_json(event)
        uid, name, pin, role = str(b.get("id", "")).strip(), str(b.get("name", "")).strip(), str(b.get("pin", "")), b.get("role")
        if not ID_RE.match(uid) or not name or not PIN_RE.match(pin):
            raise HttpError(400, "Проверьте идентификатор, имя и PIN (только цифры, от 4)")
        new = make_user(uid, name, pin, role)
        users = [u for u in users if u["id"] != uid] + [new]
        if not any(u["role"] == "admin" and u.get("active", True) for u in users):
            raise HttpError(400, "Должен остаться хотя бы один администратор")
        save_users(store, users)
        log_event(store, "user_save", who["sub"], ip, {"id": uid, "role": new["role"]})
        return resp(200, public_users(users))
    if method == "DELETE":
        uid = (event.get("queryStringParameters") or {}).get("id", "")
        if uid == who["sub"]:
            raise HttpError(400, "Себя удалить нельзя")
        rest = [u for u in users if u["id"] != uid]
        if len(rest) == len(users):
            raise HttpError(404, "Такого инженера нет")
        if not any(u["role"] == "admin" and u.get("active", True) for u in rest):
            raise HttpError(400, "Нельзя удалить последнего администратора")
        save_users(store, rest)
        log_event(store, "user_delete", who["sub"], ip, {"id": uid})
        return resp(200, public_users(rest))
    raise HttpError(405, "Метод не поддерживается")


def route_event(store, event):
    who = auth(event)
    b = body_json(event)
    typ = str(b.get("type", ""))[:32]
    if not re.match(r"^[a-z_]{2,32}$", typ):
        raise HttpError(400, "Некорректный тип события")
    data = b.get("data") if isinstance(b.get("data"), dict) else {}
    if len(json.dumps(data, ensure_ascii=False)) > 4000:
        data = {"truncated": True}
    data["machine"] = who.get("mid")
    log_event(store, typ, who["sub"], client_ip(event), data)
    return resp(200, {"ok": True})


def route_events_list(store, event):
    auth(event, admin=True)
    day = (event.get("queryStringParameters") or {}).get("date") or time.strftime("%Y-%m-%d", time.gmtime(time.time() + MSK))
    if not re.match(r"^\d{4}-\d{2}-\d{2}$", day):
        raise HttpError(400, "Некорректная дата")
    out = []
    for key in store.list("events/" + day + "/"):
        raw = store.get(key)
        if raw:
            out.append(json.loads(raw))
    out.sort(key=lambda e: e["t"])
    return resp(200, out)


def dispatch(store, event):
    method = (event.get("httpMethod") or "GET").upper()
    # API Gateway кладёт в path шаблон маршрута («/v1/{path+}»); реальный путь — в pathParams.path или url
    pp = (event.get("pathParams") or event.get("pathParameters") or {}).get("path")
    path = ("/" + pp if pp else (event.get("url") or event.get("path") or "")).split("?")[0].rstrip("/")
    try:
        if path.endswith("/ping"):
            return resp(200, {"ok": True, "time": now()})
        if path.endswith("/login") and method == "POST":
            return route_login(store, event)
        if path.endswith("/report") and method == "POST":
            return route_report(store, event)
        if path.endswith("/reports") and method == "GET":
            return route_reports_list(store, event)
        if path.endswith("/report") and method == "GET":
            return route_report_get(store, event)
        if path.endswith("/profiles"):
            if method == "GET":
                return route_profiles_get(store, event)
            if method == "PUT":
                return route_profiles_put(store, event)
        if path.endswith("/users"):
            return route_users(store, event, method)
        if path.endswith("/event") and method == "POST":
            return route_event(store, event)
        if path.endswith("/events") and method == "GET":
            return route_events_list(store, event)
        return resp(404, {"error": "Не найдено"})
    except HttpError as e:
        return resp(e.code, {"error": e.msg})


def handler(event, context):
    """Точка входа Cloud Functions."""
    token = (getattr(context, "token", None) or {}).get("access_token", "")
    store = S3Storage(os.environ["BUCKET"], token)
    try:
        return dispatch(store, event)
    except Exception as e:     # непредвиденное — не раскрываем детали клиенту
        print("ERROR", repr(e))
        return resp(500, {"error": "Внутренняя ошибка сервера"})
