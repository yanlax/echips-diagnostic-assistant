"""Хранилище сервера Echips: Yandex Object Storage (через IAM-токен функции) и локальная папка для тестов.
Ключи — строки вида "reports/Максим/2026-09-25/...json"; значения — bytes."""
import os
import urllib.error
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET


class LocalStorage:
    def __init__(self, root):
        self.root = root

    def _p(self, key):
        return os.path.join(self.root, *key.split("/"))

    def get(self, key):
        try:
            with open(self._p(key), "rb") as f:
                return f.read()
        except FileNotFoundError:
            return None

    def put(self, key, data):
        p = self._p(key)
        os.makedirs(os.path.dirname(p), exist_ok=True)
        with open(p, "wb") as f:
            f.write(data)

    def delete(self, key):
        try:
            os.remove(self._p(key))
        except FileNotFoundError:
            pass

    def list(self, prefix):
        out = []
        base = self.root
        for d, _, files in os.walk(base):
            for f in files:
                key = os.path.relpath(os.path.join(d, f), base).replace(os.sep, "/")
                if key.startswith(prefix):
                    out.append(key)
        return sorted(out)


class S3Storage:
    """Object Storage по HTTPS. Авторизация — IAM-токен сервисного аккаунта функции (X-YaCloud-SubjectToken)."""

    HOST = "https://storage.yandexcloud.net"

    def __init__(self, bucket, iam_token):
        self.bucket = bucket
        self.token = iam_token

    def _url(self, key="", query=""):
        path = "/" + self.bucket + ("/" + urllib.parse.quote(key, safe="/") if key else "")
        return self.HOST + path + (("?" + query) if query else "")

    def _req(self, method, url, data=None):
        req = urllib.request.Request(url, data=data, method=method, headers={"X-YaCloud-SubjectToken": self.token})
        return urllib.request.urlopen(req, timeout=20)

    def get(self, key):
        try:
            with self._req("GET", self._url(key)) as r:
                return r.read()
        except urllib.error.HTTPError as e:
            if e.code == 404:
                return None
            raise

    def put(self, key, data):
        with self._req("PUT", self._url(key), data) as r:
            r.read()

    def delete(self, key):
        try:
            with self._req("DELETE", self._url(key)) as r:
                r.read()
        except urllib.error.HTTPError as e:
            if e.code != 404:
                raise

    def list(self, prefix):
        out, token = [], None
        while True:
            q = "list-type=2&max-keys=1000&prefix=" + urllib.parse.quote(prefix, safe="")
            if token:
                q += "&continuation-token=" + urllib.parse.quote(token, safe="")
            with self._req("GET", self._url("", q)) as r:
                root = ET.fromstring(r.read())
            ns = {"s": root.tag.split("}")[0].strip("{")} if "}" in root.tag else {}
            pre = "s:" if ns else ""
            for c in root.findall(pre + "Contents", ns):
                out.append(c.find(pre + "Key", ns).text)
            trunc = root.find(pre + "IsTruncated", ns)
            if trunc is not None and trunc.text == "true":
                token = root.find(pre + "NextContinuationToken", ns).text
            else:
                break
        return out
