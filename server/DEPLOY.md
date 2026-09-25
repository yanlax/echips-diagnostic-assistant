# Сервер Echips (Yandex Cloud)

Облачная функция на Python (стандартная библиотека + пакет `ecdsa` для подписи аренды) за API Gateway. Данные — в приватном бакете Object Storage.
Секретов в репозитории нет: `SESSION_SECRET`, `BUCKET` и `LEASE_KEY` (приватный ключ подписи аренды, hex 64 символа; публичный — `data/lease_pub.txt` в exe) задаются переменными окружения функции.

## Что где
- Облако `cloud-drizzle-936`, каталог `echips` (`b1gpdg381tk3skgk5qgt`), зона `ru-central1-a`.
- Бакет: `echips-data-825d1082` (приватный). Ключи: `config/users.json`, `config/profiles.json`, `reports/<инженер>/<дата>/<устройство>/<файл>.json|pdf`, `events/<дата по Москве>/…` (журнал), `rl/…` (счётчики неверных PIN).
- Сервисный аккаунт `echips-server` (роли `storage.editor`, `functions.functionInvoker`) — от его имени работает функция и вызывается из шлюза.
- Функция `echips-api` (python312, 128 МБ, таймаут 20 с), шлюз `echips-gw`.
- Адрес API: `https://d5d2gp51qbo80vhgv6sn.nkhmighe.apigw.yandexcloud.net/v1/…`

## API (все, кроме /ping и /login, — с заголовком `Authorization: Bearer <token>`)
| Метод | Путь | Кто | Что |
|---|---|---|---|
| GET | /v1/ping | все | проверка связи |
| POST | /v1/login `{pin, machine_id, machine_name, app_version}` | все | вход по PIN, сессия 12 ч; в ответе `lease` — подписанная аренда на 7 суток для входа без интернета; 5 неверных попыток за 15 мин с одного IP → 429 |
| POST | /v1/report `{path, json, pdf_b64?, kind}` | инженер | сохранить отчёт; первый сегмент пути принудительно = имя из сессии |
| GET | /v1/reports, /v1/report?path= | админ | список и содержимое отчётов |
| GET / PUT | /v1/profiles | инженер / админ | профили моделей |
| GET / POST / DELETE | /v1/users | админ | инженеры (PIN хэшируется scrypt на сервере) |
| POST | /v1/event `{type, data}` | инженер | событие в журнал (начало теста и т. п.) |
| GET | /v1/events?date= | админ | журнал за день |

## Обновить код функции
```bash
mkdir fnbuild && cp server/handler.py server/storage.py fnbuild/
pip install --target fnbuild ecdsa        # один раз: библиотека подписи аренды
yc serverless function version create --function-name echips-api --runtime python312 \
  --entrypoint handler.handler --memory 128m --execution-timeout 20s --source-path fnbuild \
  --service-account-id <id аккаунта echips-server> --environment BUCKET=<бакет>,SESSION_SECRET=<секрет, 64 hex>,LEASE_KEY=<приватный ключ, 64 hex>
```
Новая версия применяется через 10–15 секунд. `SESSION_SECRET` смотреть в консоли (переменные окружения функции) — при смене все сессии сбрасываются.

## Тесты
```bash
python3 -m unittest discover -s server/tests -v
```
