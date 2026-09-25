import hashlib
import json
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
os.environ["SESSION_SECRET"] = "x" * 40
import handler as h
from storage import LocalStorage


def ev(method, path, body=None, token=None, q=None, ip="1.2.3.4"):
    e = {"httpMethod": method, "path": path, "headers": {}, "queryStringParameters": q or {},
         "requestContext": {"identity": {"sourceIp": ip}}, "body": json.dumps(body) if body is not None else ""}
    if token:
        e["headers"]["Authorization"] = "Bearer " + token
    return e


class T(unittest.TestCase):
    def setUp(self):
        self.d = tempfile.mkdtemp()
        self.s = LocalStorage(self.d)
        # перенос из приложения: sha256(salt:pin)
        users = [{"id": "alexey", "name": "Алексей", "role": "tech", "salt": "s1", "pin_hash": hashlib.sha256(b"s1:1111").hexdigest()},
                 {"id": "maksim", "name": "Максим", "role": "admin", "salt": "s2", "pin_hash": hashlib.sha256(b"s2:2222").hexdigest()}]
        h.save_users(self.s, users)

    def call(self, *a, **k):
        r = h.dispatch(self.s, ev(*a, **k))
        return r["statusCode"], json.loads(r["body"]) if r["body"] else None

    def login(self, pin):
        c, b = self.call("POST", "/v1/login", {"pin": pin, "machine_id": "M1", "machine_name": "PC"})
        return c, b

    def test_login_and_upgrade(self):
        c, b = self.login("1111")
        self.assertEqual(c, 200)
        self.assertEqual(b["user"]["role"], "tech")
        self.assertEqual(h.load_users(self.s)[0]["scheme"], "scrypt")       # хэш переведён на scrypt
        self.assertEqual(self.login("1111")[0], 200)                        # и после перехода PIN подходит
        self.assertEqual(self.login("9999")[0], 401)

    def test_lockout(self):
        for _ in range(5):
            self.assertEqual(self.login("0000")[0], 401)
        self.assertEqual(self.login("1111")[0], 429)                        # даже верный PIN после 5 ошибок
        self.assertEqual(self.call("POST", "/v1/login", {"pin": "1111"}, ip="9.9.9.9")[0], 200)  # другой адрес — можно

    def test_report_roles_and_path(self):
        tok = self.login("1111")[1]["token"]
        adm = self.login("2222")[1]["token"]
        rep = {"report": {"x": 1}}
        body = {"path": "Чужой/2026-09-25/4654_SN123/130037_before_full", "json": json.dumps(rep), "kind": "auto"}
        c, r = self.call("POST", "/v1/report", body, token=tok)
        self.assertEqual(c, 200)
        self.assertEqual(r["path"], "Алексей/2026-09-25/4654_SN123/130037_before_full")   # имя подменено на имя из сессии
        self.assertEqual(self.call("GET", "/v1/reports", token=tok)[0], 403)              # техник историю не видит
        c, lst = self.call("GET", "/v1/reports", token=adm)
        self.assertEqual((c, len(lst), lst[0]["engineer"], lst[0]["device"]), (200, 1, "Алексей", "4654_SN123"))
        c, got = self.call("GET", "/v1/report", token=adm, q={"path": r["path"] + ".json"})
        self.assertEqual((c, got), (200, rep))
        self.assertEqual(self.call("POST", "/v1/report", {"path": "../x", "json": "{}"}, token=tok)[0], 400)
        self.assertEqual(self.call("POST", "/v1/report", {"path": "a/2026-09-25/b/c", "json": "{}"})[0], 401)

    def test_users_admin(self):
        adm = self.login("2222")[1]["token"]
        tech = self.login("1111")[1]["token"]
        self.assertEqual(self.call("GET", "/v1/users", token=tech)[0], 403)
        c, lst = self.call("POST", "/v1/users", {"id": "new1", "name": "Новый", "pin": "5555", "role": "admin"}, token=adm)
        self.assertEqual(c, 200)
        self.assertNotIn("pin_hash", json.dumps(lst))
        self.assertEqual(self.login("5555")[1]["user"]["role"], "admin")
        self.assertEqual(self.call("DELETE", "/v1/users", token=adm, q={"id": "maksim"})[0], 400)   # себя нельзя
        self.assertEqual(self.call("DELETE", "/v1/users", token=adm, q={"id": "alexey"})[0], 200)
        self.assertEqual(self.login("1111")[0], 401)

    def test_events_profiles(self):
        adm = self.login("2222")[1]["token"]
        tech = self.login("1111")[1]["token"]
        self.assertEqual(self.call("POST", "/v1/event", {"type": "test_start", "data": {"id": "sys"}}, token=tech)[0], 200)
        c, evs = self.call("GET", "/v1/events", token=adm)
        self.assertEqual(c, 200)
        self.assertIn("test_start", [e["type"] for e in evs])
        self.assertEqual(self.call("PUT", "/v1/profiles", {"key": "NB1", "profile": {"name": "n"}}, token=tech)[0], 403)
        c, p = self.call("PUT", "/v1/profiles", {"key": "NB1", "profile": {"name": "n"}}, token=adm)
        self.assertEqual((c, p["version"]), (200, 1))
        self.assertEqual(self.call("GET", "/v1/profiles", token=tech)[1]["models"]["NB1"]["name"], "n")

    def test_gateway_path_forms(self):
        for extra in ({"path": "/v1/{path+}", "pathParams": {"path": "ping"}}, {"path": "/v1/{path+}", "url": "/v1/ping?x=1"}):
            e = ev("GET", "/x"); e.update(extra)
            self.assertEqual(h.dispatch(self.s, e)["statusCode"], 200)

    @unittest.skipIf(h.SigningKey is None, "нужен пакет ecdsa (pip install ecdsa)")
    def test_lease(self):
        os.environ["LEASE_KEY"] = "0101010101010101010101010101010101010101010101010101010101010101"
        try:
            c, b = self.login("1111")
        finally:
            del os.environ["LEASE_KEY"]
        self.assertEqual(c, 200)
        p = json.loads(b["lease"]["payload"])
        self.assertEqual((p["sub"], p["role"], p["iters"]), ("alexey", "tech", h.LEASE_ITERS))
        self.assertEqual(hashlib.pbkdf2_hmac("sha256", b"1111", bytes.fromhex(p["salt"]), p["iters"]).hex(), p["verifier"])
        self.assertGreater(p["exp"] - p["iat"], 6 * 86400)
        self.assertNotIn("lease", self.login("2222")[1])          # без ключа аренда не выдаётся

    def test_token_expiry_and_tamper(self):
        tok = self.login("1111")[1]["token"]
        bad = tok[:-2] + ("aa" if not tok.endswith("aa") else "bb")
        self.assertEqual(self.call("GET", "/v1/profiles", token=bad)[0], 401)
        user = {"id": "alexey", "name": "А", "role": "tech"}
        old, _ = h.make_token(user, "M", ttl=-10)
        self.assertEqual(self.call("GET", "/v1/profiles", token=old)[0], 401)


if __name__ == "__main__":
    unittest.main()
