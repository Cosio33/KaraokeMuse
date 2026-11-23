#!/usr/bin/env node
// Caracterización de las funciones PURAS de server/auth.js contra el
// Node original (ADR-013). Las funciones de sesión (createSession,
// getUserFromToken, destroySession) toman un DatabaseSync vivo y se
// prueban con tests de integración en Rust (tests/sessions.rs),
// no con este JSONL.
//
// Uso: node rust/auth-record.mjs  (desde la raíz del repo)
// Escribe: rust/auth-cases.jsonl (un caso por línea)
// Limpia: rust/auth-record-data/ (BD temporal, nunca data/).
import { openDatabase, seed } from '../server/db.js';
import { hashPassword, verifyPassword, publicUser } from '../server/auth.js';
import crypto from 'node:crypto';
import fs from 'node:fs';

const DATA = 'rust/auth-record-data';
const OUT = 'rust/auth-cases.jsonl';
fs.rmSync(DATA, { recursive: true, force: true });

const db = openDatabase(DATA);
seed(db, {
  adminUser: 'admin',
  adminPassword: 'AdminTest123!',
  roomName: 'Sala principal',
  roomPassword: null,
});

const cases = [];
const add = (fn, args, ok) => cases.push({ fn, args, ok });

// --- verifyPassword: vector fijo (sal conocida, hash de Node) ---
// OJO (ADR-015): `verifyPassword` pasa la sal hex COMO CADENA a
// `scryptSync`, así que el vector se genera con la cadena, no con
// `Buffer.from(saltHex, 'hex')` (eso era la convención equivocada
// que hizo pasar el replay 9/9 sin detectar el bug de login).
const saltHex = '000102030405060708090a0b0c0d0e0f';
const hashHex = crypto.scryptSync('Secreta123!', saltHex, 64).toString('hex');
add('verifyPassword', ['Secreta123!', saltHex, hashHex], true);
add('verifyPassword', ['secreta123!', saltHex, hashHex], false);
add('verifyPassword', ['Secreta123!', '', hashHex], false);
add('verifyPassword', ['Secreta123!', saltHex, ''], false);
add('verifyPassword', ['Secreta123!', 'zz', hashHex], false);
add('verifyPassword', ['Secreta123!', saltHex, hashHex.slice(0, 64)], false);

// --- hashPassword: forma (sal 32 hex + hash 128 hex; aleatorio) ---
const hp = hashPassword('Secreta123!');
if (hp.salt.length !== 32 || hp.hash.length !== 128) {
  throw new Error('hashPassword con forma inesperada: ' + JSON.stringify(hp));
}
if (!verifyPassword('Secreta123!', hp.salt, hp.hash)) {
  throw new Error('hashPassword no verifica contra sí mismo');
}
add('hashPassword', ['Secreta123!'], { $shape: 'salt32+hash128' });

// --- publicUser: fila real del admin sembrado + null ---
const admin = db
  .prepare('SELECT id, username, role, created_at FROM users WHERE role = ?')
  .get('admin');
add('publicUser', [admin], publicUser(admin));
add('publicUser', [null], publicUser(null));

fs.writeFileSync(OUT, cases.map((c) => JSON.stringify(c)).join('\n') + '\n');
console.log(`${cases.length} casos escritos en ${OUT}`);
db.close?.();
fs.rmSync(DATA, { recursive: true, force: true });
