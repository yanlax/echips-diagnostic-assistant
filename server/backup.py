"""Ночная резервная копия бакета Echips (Yandex Cloud Functions, по таймеру).
Копирует во второй бакет (BACKUP_BUCKET) всё новое и изменённое; ничего не удаляет — удалённые из основного
бакета файлы в копии остаются. В бакете копии включено версионирование: перезаписанные файлы (список инженеров,
профили) хранятся в предыдущих версиях."""
import os

from storage import S3Storage


def sync(src, dst):
    have = {o["key"]: o["etag"] for o in dst.list_meta("")}
    copied = skipped = 0
    for o in src.list_meta(""):
        if have.get(o["key"]) == o["etag"]:
            skipped += 1
            continue
        data = src.get(o["key"])
        if data is not None:
            dst.put(o["key"], data)
            copied += 1
    return copied, skipped


def handler(event, context):
    token = (getattr(context, "token", None) or {}).get("access_token", "")
    src = S3Storage(os.environ["BUCKET"], token)
    dst = S3Storage(os.environ["BACKUP_BUCKET"], token)
    copied, skipped = sync(src, dst)
    print("BACKUP copied=%d unchanged=%d" % (copied, skipped))
    return {"statusCode": 200, "body": "copied=%d unchanged=%d" % (copied, skipped)}
