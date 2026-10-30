# Support QR codes

Images for the About tab's support list (`app/Sources/WhoateKit/Links.swift`,
`Links.support`). An entry `.qrCode(resource: "<file name>.png")` shows the PNG
of that name from this folder; `scripts/bundle-app.sh` copies every `*.png`
here into `Contents/Resources/Support/`.

`wechat-pay.png` is the owner's WeChat Pay code (owner 2026-10-27 11:49; shown in both
languages since 2026-10-30), cropped
from the card WeChat exports so it stays readable at 64 pt; `site/support/` holds the
same file for the product page. Never add a placeholder or sample image.
