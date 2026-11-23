#!/bin/sh
# Arranca el servidor ORIGINAL para `rust-anything app record --http`.
#
# Siembra una BD de pruebas DETERMINISTA en rust/record-data (nunca
# data/, que es la BD real) y luego sirve el Node original en $PORT
# (variable que fija rust-anything; HOST por defecto 0.0.0.0).
# Cada arranque vuelve a sembrar desde cero: las dos corridas de
# `app record` ven el mismo estado inicial.
#
# Uso (lo invoca rust-anything): sh rust/record-server.sh
set -e
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export DATA_DIR="$ROOT/rust/record-data"
export KARAOKE_ADMIN_USER=admin
export KARAOKE_ADMIN_PASSWORD='AdminTest123!'
export KARAOKE_ROOM_NAME='Sala principal'
node "$ROOT/rust/seed-record-db.mjs" "$DATA_DIR"
exec node "$ROOT/server/index.js"
