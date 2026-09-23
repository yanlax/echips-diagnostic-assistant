# Приём отчётов (Cloudflare Worker)

Приложение шлёт отчёт сюда, Worker кладёт его JSON-файлом в приватный
репозиторий `yanlax/echips-reports` (`reports/ГГГГ-ММ/<время>_<SN>_<инженер>_<auto|manual>.json`).

## Развёртывание (один раз)

1. Токен GitHub: https://github.com/settings/personal-access-tokens/new —
   fine-grained, Only select repositories → `echips-reports`,
   Permissions → Contents: Read and write.
2. Аккаунт на https://dash.cloudflare.com (бесплатного достаточно), затем в
   этой папке:
   ```
   npm i -g wrangler
   wrangler login
   wrangler secret put GITHUB_TOKEN   # вставить токен из шага 1
   wrangler deploy
   ```
3. `wrangler deploy` выведет адрес вида `https://echips-reports.<аккаунт>.workers.dev` —
   его нужно прописать в `src-tauri/src/commands/upload.rs` (константа
   `REPORTS_URL`) и выпустить релиз.
4. Cloudflare → Security → WAF → Rate limiting rules: ограничить POST на этот
   адрес (например, 30 запросов в минуту с одного IP).
