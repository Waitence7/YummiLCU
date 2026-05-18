#!/usr/bin/env bash
# waitence.kro.kr Let's Encrypt (kro.kr 주간 한도 해제 후 실행)
set -euo pipefail
DOMAIN=yummi.duckdns.org
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "DNS 확인 (168.107.42.241 이어야 함):"
dig +short "$DOMAIN" A @8.8.8.8 || true

sudo mkdir -p /var/www/certbot
sudo certbot certonly --standalone -d "$DOMAIN" \
  --non-interactive --agree-tos --register-unsafely-without-email

sudo cp "$SCRIPT_DIR/nginx-waitence.conf" /etc/nginx/sites-available/waitence
sudo ln -sf /etc/nginx/sites-available/waitence /etc/nginx/sites-enabled/waitence
sudo rm -f /etc/nginx/sites-enabled/default
sudo nginx -t && sudo systemctl reload nginx
echo "OK: https://$DOMAIN"
