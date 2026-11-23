# KaraokeMuse v1 — pruebas de caracterización (35 checks).
#
# Reconstrucción de /tmp/km-test.py, que se perdió (vivía en /tmp y no
# estaba en el repo). La cobertura sigue TESTS.md al pie de la letra:
# salud, frontend estático, QR vendored, 401 sin token, login admin
# bueno/malo, unirse con código bueno/malo, alta de enlaces YouTube y
# Spotify como invitado, edición como invitado -> 403, registro de vídeo
# local como invitado -> 403, borrar canción ajena -> 403, borrar propia
# -> 200, registro de vídeo local como admin (existe -> 201, inexistente
# -> 400), streaming por rangos (206) y completo (200), cola (agregar,
# quitar ajena -> 403, siguiente como invitado -> 403, siguiente como
# admin -> 200 y Player actualizado), aleatorias (3 distintas),
# solicitudes (crear, invitado cambia estado -> 403, admin cambia a
# in_progress, invitado solo ve las suyas, admin ve todas) y filtro
# minRating=5. Los últimos 5 checks (sala, lista de canciones, lista de
# cola, /api/me con token, logout) completan los 35 y son comprobaciones
# naturales de la misma API.
#
# Uso:
#   PORT=8081 DATA_DIR=/tmp/km-char-data \
#     KARAOKE_ADMIN_USER=admin KARAOKE_ADMIN_PASSWORD='AdminTest123!' \
#     node server/index.js &
#   python3 tests/characterization.py
#     --base http://127.0.0.1:8081 --room-code XXXXXX \
#     --admin-password AdminTest123! --fake-video /tmp/km-char-data/fake.mp4
#
# El servidor debe arrancar con una base limpia. El vídeo falso debe
# medir 42 bytes y empezar por b'fake-mp4-c' (bytes 0-9).
# Salida: "N/35 PASARON" y exit 0 solo si pasan todos.
import argparse
import json
import sys
import urllib.request
import urllib.error

CHECKS = []


def check(name):
    def deco(fn):
        CHECKS.append((name, fn))
        return fn
    return deco


class Client:
    def __init__(self, base):
        self.base = base

    def req(self, method, path, token=None, body=None, headers=None):
        url = self.base + path
        data = json.dumps(body).encode() if body is not None else None
        r = urllib.request.Request(url, data=data, method=method)
        r.add_header('content-type', 'application/json')
        if token:
            r.add_header('authorization', f'Bearer {token}')
        for k, v in (headers or {}).items():
            r.add_header(k, v)
        try:
            with urllib.request.urlopen(r) as resp:
                raw = resp.read()
                return resp.status, raw, dict(resp.headers)
        except urllib.error.HTTPError as e:
            return e.code, e.read(), dict(e.headers)

    def jreq(self, method, path, token=None, body=None, headers=None):
        status, raw, h = self.req(method, path, token, body, headers)
        try:
            parsed = json.loads(raw.decode())
        except Exception:
            parsed = None
        return status, parsed, raw, h


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--base', default='http://127.0.0.1:8081')
    ap.add_argument('--room-code', required=True)
    ap.add_argument('--admin-password', default='AdminTest123!')
    ap.add_argument('--fake-video', required=True)
    args = ap.parse_args()
    c = Client(args.base)
    S = {}  # estado compartido entre checks

    @check('01 salud')
    def _():
        s, j, _, _ = c.jreq('GET', '/api/health')
        assert s == 200 and j == {'ok': True, 'app': 'KaraokeMuse', 'version': '1.0.0'}, (s, j)

    @check('02 frontend estático')
    def _():
        s, raw, h = c.req('GET', '/')
        assert s == 200 and b'KaraokeMuse' in raw, (s, raw[:80])

    @check('03 QR vendored')
    def _():
        s, raw, h = c.req('GET', '/vendor/qrcode.min.js')
        assert s == 200 and len(raw) > 1000, (s, len(raw))

    @check('04 401 sin token')
    def _():
        s, j, _, _ = c.jreq('GET', '/api/me')
        assert s == 401, (s, j)

    @check('05 login admin bueno')
    def _():
        s, j, _, _ = c.jreq('POST', '/api/auth/login',
                            body={'username': 'admin', 'password': args.admin_password})
        assert s == 200 and j.get('token'), (s, j)
        S['admin'] = j['token']

    @check('06 login admin malo')
    def _():
        s, j, _, _ = c.jreq('POST', '/api/auth/login',
                            body={'username': 'admin', 'password': 'no-es-esta'})
        assert s == 401, (s, j)

    @check('07 unirse con código bueno')
    def _():
        s, j, _, _ = c.jreq('POST', '/api/join',
                            body={'code': args.room_code, 'name': 'InvitadoA'})
        assert s == 200 and j.get('token') and j['user']['role'] == 'guest', (s, j)
        S['guestA'] = j['token']

    @check('08 unirse con código malo')
    def _():
        s, j, _, _ = c.jreq('POST', '/api/join',
                            body={'code': 'XXXXXX', 'name': 'Nadie'})
        assert s == 404, (s, j)

    @check('09 alta enlace YouTube como invitado')
    def _():
        s, j, _, _ = c.jreq('POST', '/api/songs', token=S['guestA'], body={
            'title': 'Never Gonna Give You Up', 'artist': 'Rick Astley',
            'platform': 'youtube', 'url': 'https://www.youtube.com/watch?v=dQw4w9WgXcQ',
            'rating': 5})
        assert s == 201 and j['song']['platform'] == 'youtube', (s, j)
        S['yt'] = j['song']['id']

    @check('10 alta enlace Spotify como invitado')
    def _():
        s, j, _, _ = c.jreq('POST', '/api/songs', token=S['guestA'], body={
            'title': 'Canción Spotify', 'artist': 'Artista',
            'platform': 'spotify', 'url': 'https://open.spotify.com/track/1234567',
            'rating': 0})
        assert s == 201 and j['song']['platform'] == 'spotify', (s, j)
        S['sp'] = j['song']['id']

    @check('11 edición como invitado -> 403')
    def _():
        s, j, _, _ = c.jreq('PATCH', f"/api/songs/{S['yt']}", token=S['guestA'],
                            body={'title': 'Cambio no permitido'})
        assert s == 403, (s, j)

    @check('12 registro vídeo local como invitado -> 403')
    def _():
        s, j, _, _ = c.jreq('POST', '/api/songs', token=S['guestA'], body={
            'title': 'Local invitado', 'platform': 'local',
            'local_path': args.fake_video})
        assert s == 403, (s, j)

    @check('13 borrar canción ajena -> 403')
    def _():
        s, j, _, _ = c.jreq('POST', '/api/join',
                            body={'code': args.room_code, 'name': 'InvitadoB'})
        assert s == 200, (s, j)
        S['guestB'] = j['token']
        s, j, _, _ = c.jreq('DELETE', f"/api/songs/{S['yt']}", token=S['guestB'])
        assert s == 403, (s, j)

    @check('14 borrar canción propia -> 200')
    def _():
        s, j, _, _ = c.jreq('POST', '/api/songs', token=S['guestA'], body={
            'title': 'Borrarme', 'platform': 'youtube',
            'url': 'https://www.youtube.com/watch?v=dQw4w9WgXcQ'})
        assert s == 201, (s, j)
        own = j['song']['id']
        s, j, _, _ = c.jreq('DELETE', f'/api/songs/{own}', token=S['guestA'])
        assert s == 200 and j.get('ok') is True, (s, j)

    @check('15 registro vídeo local como admin (existe) -> 201')
    def _():
        s, j, _, _ = c.jreq('POST', '/api/songs', token=S['admin'], body={
            'title': 'Vídeo local', 'platform': 'local',
            'local_path': args.fake_video, 'rating': 5})
        assert s == 201 and j['song']['platform'] == 'local', (s, j)
        S['local'] = j['song']['id']

    @check('16 registro vídeo local como admin (inexistente) -> 400')
    def _():
        s, j, _, _ = c.jreq('POST', '/api/songs', token=S['admin'], body={
            'title': 'Vídeo fantasma', 'platform': 'local',
            'local_path': '/tmp/km-char-data/no-existe.mp4'})
        assert s == 400, (s, j)

    @check('17 streaming por rangos -> 206')
    def _():
        s, raw, h = c.req('GET', f"/api/media/local/{S['local']}",
                          token=S['admin'], headers={'Range': 'bytes=0-9'})
        assert s == 206 and raw == b'fake-mp4-c', (s, raw)
        cr = next((v for k, v in h.items() if k.lower() == 'content-range'), '')
        assert cr.startswith('bytes 0-9/42'), h

    @check('18 streaming completo -> 200')
    def _():
        s, raw, h = c.req('GET', f"/api/media/local/{S['local']}", token=S['admin'])
        assert s == 200 and len(raw) == 42, (s, len(raw))

    @check('19 cola: agregar -> 201')
    def _():
        s, j, _, _ = c.jreq('POST', '/api/queue', token=S['guestA'],
                            body={'songId': S['yt']})
        assert s == 201 and j.get('queueId'), (s, j)
        S['q1'] = j['queueId']

    @check('20 cola: quitar ajena -> 403')
    def _():
        s, j, _, _ = c.jreq('DELETE', f"/api/queue/{S['q1']}", token=S['guestB'])
        assert s == 403, (s, j)

    @check('21 cola: siguiente como invitado -> 403')
    def _():
        s, j, _, _ = c.jreq('POST', '/api/queue/next', token=S['guestA'])
        assert s == 403, (s, j)

    @check('22 cola: siguiente como admin -> 200')
    def _():
        s, j, _, _ = c.jreq('POST', '/api/queue/next', token=S['admin'])
        assert s == 200 and j.get('song', {}).get('id') == S['yt'], (s, j)

    @check('23 Player actualizado tras avanzar cola')
    def _():
        s, j, _, _ = c.jreq('GET', '/api/player', token=S['admin'])
        assert s == 200 and j.get('song', {}).get('id') == S['yt'], (s, j)

    @check('24 aleatorias: 3 distintas')
    def _():
        s, j, _, _ = c.jreq('GET', '/api/songs/random?count=3', token=S['admin'])
        assert s == 200 and len(j['songs']) == 3, (s, j)
        assert len({x['id'] for x in j['songs']}) == 3, j

    @check('25 solicitudes: crear -> 201')
    def _():
        s, j, _, _ = c.jreq('POST', '/api/requests', token=S['guestA'], body={
            'video_url': 'https://www.youtube.com/watch?v=dQw4w9WgXcQ',
            'title': 'Petición A', 'difficulty': 'facil'})
        assert s == 201 and j['request']['status'] == 'pending', (s, j)
        S['reqA'] = j['request']['id']
        s, j, _, _ = c.jreq('POST', '/api/requests', token=S['guestB'], body={
            'video_url': 'https://www.youtube.com/watch?v=dQw4w9WgXcQ',
            'title': 'Petición B'})
        assert s == 201, (s, j)
        S['reqB'] = j['request']['id']

    @check('26 solicitudes: invitado cambia estado -> 403')
    def _():
        s, j, _, _ = c.jreq('PATCH', f"/api/requests/{S['reqA']}", token=S['guestA'],
                            body={'status': 'in_progress'})
        assert s == 403, (s, j)

    @check('27 solicitudes: admin cambia a in_progress -> 200')
    def _():
        s, j, _, _ = c.jreq('PATCH', f"/api/requests/{S['reqA']}", token=S['admin'],
                            body={'status': 'in_progress'})
        assert s == 200 and j['request']['status'] == 'in_progress', (s, j)

    @check('28 solicitudes: invitado solo ve las suyas')
    def _():
        s, j, _, _ = c.jreq('GET', '/api/requests', token=S['guestA'])
        assert s == 200, (s, j)
        assert len(j['requests']) == 1 and j['requests'][0]['id'] == S['reqA'], j

    @check('29 solicitudes: admin ve todas')
    def _():
        s, j, _, _ = c.jreq('GET', '/api/requests', token=S['admin'])
        assert s == 200 and len(j['requests']) == 2, (s, j)

    @check('30 filtro minRating=5')
    def _():
        s, j, _, _ = c.jreq('GET', '/api/songs?minRating=5', token=S['admin'])
        assert s == 200, (s, j)
        assert len(j['songs']) == 2, j  # YT (5) y vídeo local (5); Spotify tiene 0
        assert all(x['rating'] >= 5 for x in j['songs']), j

    @check('31 sala: GET /api/room')
    def _():
        s, j, _, _ = c.jreq('GET', '/api/room', token=S['admin'])
        assert s == 200 and j.get('code') == args.room_code, (s, j)

    @check('32 lista de canciones contiene las altas')
    def _():
        s, j, _, _ = c.jreq('GET', '/api/songs', token=S['admin'])
        assert s == 200, (s, j)
        ids = {x['id'] for x in j['songs']}
        assert {S['yt'], S['sp'], S['local']} <= ids, ids

    @check('33 lista de cola tras avanzar: vacía')
    def _():
        s, j, _, _ = c.jreq('GET', '/api/queue', token=S['admin'])
        assert s == 200 and j['queue'] == [], (s, j)

    @check('34 /api/me con token')
    def _():
        s, j, _, _ = c.jreq('GET', '/api/me', token=S['admin'])
        assert s == 200 and j['user']['role'] == 'admin', (s, j)

    @check('35 logout -> 200 y /api/me -> 401')
    def _():
        s, j, _, _ = c.jreq('POST', '/api/auth/logout', token=S['admin'])
        assert s == 200 and j.get('ok') is True, (s, j)
        s, j, _, _ = c.jreq('GET', '/api/me', token=S['admin'])
        assert s == 401, (s, j)

    passed = 0
    failures = []
    for name, fn in CHECKS:
        try:
            fn()
            passed += 1
            print(f'PASS {name}')
        except Exception as e:
            failures.append(name)
            print(f'FAIL {name}: {e}')
    print(f'\n{passed}/{len(CHECKS)} PASARON')
    sys.exit(0 if passed == len(CHECKS) else 1)


if __name__ == '__main__':
    main()
