// Cloudflare Worker: приём отчётов от приложения Echips Hardware Check и
// запись их в приватный репозиторий отчётов через GitHub Contents API.
// Токен GitHub хранится только здесь (секрет GITHUB_TOKEN), в приложении его нет.
//
// Защита от мусора: только POST, JSON до MAX_BYTES, ожидаемая форма отчёта,
// путь файла строится здесь (клиент им не управляет). Частоту запросов
// ограничьте правилом Cloudflare (Security → WAF → Rate limiting).

const MAX_BYTES = 2_000_000;

function safe(s, max) {
  return String(s || "unknown").replace(/[^A-Za-z0-9А-Яа-яЁё._-]+/g, "_").slice(0, max);
}

function toBase64(text) {
  const bytes = new TextEncoder().encode(text);
  let bin = "";
  for (let i = 0; i < bytes.length; i += 0x8000) {
    bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(bin);
}

export default {
  async fetch(request, env) {
    if (request.method !== "POST") return new Response("method not allowed", { status: 405 });

    const text = await request.text();
    if (text.length > MAX_BYTES) return new Response("too large", { status: 413 });

    let body;
    try {
      body = JSON.parse(text);
    } catch {
      return new Response("bad json", { status: 400 });
    }
    const report = body && body.report;
    if (
      !["auto", "manual"].includes(body && body.kind) ||
      !report || typeof report !== "object" ||
      !Array.isArray(report.results) || typeof report.device_model !== "string"
    ) {
      return new Response("bad report", { status: 400 });
    }

    const now = new Date();
    const stamp = now.toISOString().replace(/[:.]/g, "-").slice(0, 19);
    const path = `reports/${now.toISOString().slice(0, 7)}/${stamp}_${safe(report.device_serial, 30)}_${safe(report.engineer, 30)}_${body.kind}.json`;

    const resp = await fetch(
      `https://api.github.com/repos/${env.REPO}/contents/${path.split("/").map(encodeURIComponent).join("/")}`,
      {
        method: "PUT",
        headers: {
          Authorization: `Bearer ${env.GITHUB_TOKEN}`,
          "User-Agent": "echips-reports-worker",
          Accept: "application/vnd.github+json",
        },
        body: JSON.stringify({
          message: `Отчёт: ${safe(report.device_model, 40)} / ${safe(report.engineer, 30)} (${body.kind})`,
          content: toBase64(JSON.stringify(body, null, 2)),
        }),
      }
    );
    return new Response(resp.ok ? "ok" : "github error", { status: resp.ok ? 200 : 502 });
  },
};
