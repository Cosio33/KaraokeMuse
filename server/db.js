// KaraokeMuse — persistencia SQLite (node:sqlite, sin dependencias externas).
// Tablas: users, rooms, songs, queue, requests, sessions, player_state.
import { DatabaseSync } from 'node:sqlite';
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { hashPassword } from './auth.js';

export function randomCode(length = 6) {
  const chars = 'ABCDEFGHJKMNPQRSTUVWXYZ23456789'; // sin 0/O/1/I para dictarlo fácil
  let out = '';
  const buf = crypto.randomBytes(length);
  for (const b of buf) out += chars[b % chars.length];
  return out;
}

export function newId(prefix) {
  return `${prefix}_${crypto.randomBytes(8).toString('hex')}`;
}

export function openDatabase(dataDir) {
  fs.mkdirSync(dataDir, { recursive: true });
  const db = new DatabaseSync(path.join(dataDir, 'karaokemuse.sqlite'));
  db.exec('PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;');
  db.exec(`
    CREATE TABLE IF NOT EXISTS users (
      id TEXT PRIMARY KEY,
      username TEXT UNIQUE NOT NULL,
      password_hash TEXT,
      salt TEXT,
      role TEXT NOT NULL CHECK (role IN ('admin','guest')),
      created_at TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS rooms (
      id TEXT PRIMARY KEY,
      name TEXT NOT NULL,
      code TEXT UNIQUE NOT NULL,
      password_hash TEXT,
      salt TEXT,
      created_by TEXT,
      created_at TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS songs (
      id TEXT PRIMARY KEY,
      title TEXT NOT NULL,
      artist TEXT NOT NULL DEFAULT '',
      platform TEXT NOT NULL CHECK (platform IN ('youtube','spotify','local')),
      url TEXT NOT NULL DEFAULT '',
      local_path TEXT NOT NULL DEFAULT '',
      thumbnail_url TEXT NOT NULL DEFAULT '',
      genre TEXT NOT NULL DEFAULT '',
      language TEXT NOT NULL DEFAULT '',
      difficulty TEXT NOT NULL DEFAULT 'media' CHECK (difficulty IN ('facil','media','dificil')),
      status TEXT NOT NULL DEFAULT 'want' CHECK (status IN ('want','practicing','mastered')),
      rating INTEGER NOT NULL DEFAULT 0 CHECK (rating BETWEEN 0 AND 5),
      is_favorite INTEGER NOT NULL DEFAULT 0,
      notes TEXT NOT NULL DEFAULT '',
      lyrics_text TEXT NOT NULL DEFAULT '',
      owner_user_id TEXT,
      created_at TEXT NOT NULL,
      updated_at TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS queue (
      id TEXT PRIMARY KEY,
      room_id TEXT NOT NULL,
      song_id TEXT NOT NULL,
      user_id TEXT NOT NULL,
      position INTEGER NOT NULL,
      created_at TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS requests (
      id TEXT PRIMARY KEY,
      video_url TEXT NOT NULL,
      title TEXT NOT NULL,
      artist TEXT NOT NULL DEFAULT '',
      difficulty TEXT NOT NULL DEFAULT 'media' CHECK (difficulty IN ('facil','media','dificil')),
      language TEXT NOT NULL DEFAULT '',
      notes TEXT NOT NULL DEFAULT '',
      status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','in_progress','done')),
      requester_user_id TEXT,
      admin_note TEXT NOT NULL DEFAULT '',
      created_at TEXT NOT NULL,
      updated_at TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS sessions (
      token_hash TEXT PRIMARY KEY,
      user_id TEXT NOT NULL,
      expires_at TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS player_state (
      room_id TEXT PRIMARY KEY,
      current_song_id TEXT,
      updated_at TEXT NOT NULL
    );
  `);
  return db;
}

// Crea el admin y la sala por defecto si no existen. Devuelve { adminCreated, room }.
export function seed(db, opts) {
  const now = new Date().toISOString();
  let adminCreated = false;
  const admin = db.prepare('SELECT * FROM users WHERE role = ?').get('admin');
  if (!admin) {
    const { salt, hash } = hashPassword(opts.adminPassword);
    db.prepare('INSERT INTO users (id, username, password_hash, salt, role, created_at) VALUES (?, ?, ?, ?, ?, ?)')
      .run(newId('u'), opts.adminUser, hash, salt, 'admin', now);
    adminCreated = true;
  }
  let room = db.prepare('SELECT * FROM rooms ORDER BY created_at LIMIT 1').get();
  if (!room) {
    const id = newId('room');
    const code = randomCode(6);
    let passwordHash = null, salt = null;
    if (opts.roomPassword) {
      const h = hashPassword(opts.roomPassword);
      salt = h.salt; passwordHash = h.hash;
    }
    db.prepare('INSERT INTO rooms (id, name, code, password_hash, salt, created_by, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)')
      .run(id, opts.roomName, code, passwordHash, salt, null, now);
    room = db.prepare('SELECT * FROM rooms WHERE id = ?').get(id);
  }
  db.prepare('INSERT OR IGNORE INTO player_state (room_id, current_song_id, updated_at) VALUES (?, NULL, ?)')
    .run(room.id, now);
  return { adminCreated, room };
}
