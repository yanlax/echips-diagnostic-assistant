"""Сборка самодостаточного портала: вшивает шрифты и логотип (base64) в template.html.
Результат: service-portal.html (полный HTML-файл) и service-portal.artifact.html (тело для публикации в Artifact)."""
import base64, re, os
HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(HERE, '..', '..', 'src')
def b64(p): return base64.b64encode(open(p, 'rb').read()).decode()
faces = []
def face(fam, weight, file, rng):
    faces.append("@font-face{font-family:'%s';font-style:normal;font-weight:%s;font-display:swap;src:url(data:font/woff2;base64,%s) format('woff2');unicode-range:%s}" % (fam, weight, b64(SRC + '/fonts/' + file), rng))
css = open(SRC + '/fonts/fonts.css', encoding='utf-8').read()
for m in re.finditer(r"font-family:\s*'([^']+)';[^}]*?font-weight:\s*([^;]+);[^}]*?url\(([^)]+)\)[^}]*?unicode-range:\s*([^;]+);", css):
    face(m.group(1), m.group(2).strip(), m.group(3), m.group(4).strip())
t = open(os.path.join(HERE, 'template.html'), encoding='utf-8').read()
t = t.replace('/*FONTS*/', '\n'.join(faces)).replace('%%LOGO%%', 'data:image/png;base64,' + b64(SRC + '/logo.png'))
open(os.path.join(HERE, 'service-portal.artifact.html'), 'w', encoding='utf-8').write(t)
full = '<!doctype html>\n<html lang="ru">\n<head>\n<meta charset="utf-8">\n<meta name="viewport" content="width=device-width,initial-scale=1,viewport-fit=cover">\n' + t.replace('<title>', '<title>', 1) + '\n</body>\n</html>\n'
# заголовок и стили — в head, остальное — в body
i = full.index('</style>') + len('</style>')
full = full[:i] + '\n</head>\n<body>' + full[i:]
open(os.path.join(HERE, 'service-portal.html'), 'w', encoding='utf-8').write(full)
print('ok', os.path.getsize(os.path.join(HERE, 'service-portal.html')) // 1024, 'КБ')
