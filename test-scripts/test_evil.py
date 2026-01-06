import socket
import time
import json
import random

SERVER_IP = "127.0.0.1"
SERVER_PORT = 8888
MY_ID = "Evil_" + str(random.randint(1, 100))

sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)

print(f"我是 {MY_ID}, 启动恶意测试客户端（会在随机时刻发送不合规位置）")

# local true state (the client believes these are real)
x, y, z = 0.0, 0.0, 0.0
rx, ry, rz = 0.0, 0.0, 0.0
vx, vy, vz = 0.2, 0.0, 0.0

try:
    while True:
        # integrate true state
        x += vx * 0.5
        y += vy * 0.5
        z += vz * 0.5

        # small random variations in velocity
        if random.random() < 0.2:
            vx += random.uniform(-0.02, 0.02)
            vy += random.uniform(-0.02, 0.02)
            vz += random.uniform(-0.02, 0.02)

        ts = int(time.time() * 1000)

        # with some probability, send a spoofed (invalid) position far beyond velocity limits
        is_spoof = random.random() < 0.15
        if is_spoof:
            # create a bogus position far away (10..50 meters) but do NOT update local true state
            fake_x = x + random.uniform(10.0, 50.0)
            fake_y = y + random.uniform(10.0, 50.0)
            fake_z = z + random.uniform(10.0, 50.0)
            send = {
                "id": MY_ID,
                "x": fake_x,
                "y": fake_y,
                "z": fake_z,
                "rx": rx,
                "ry": ry,
                "rz": rz,
                "vx": vx,
                "vy": vy,
                "vz": vz,
                "ts": ts,
            }
            print(f"[SEND SPOOF] pos=({fake_x:.2f},{fake_y:.2f},{fake_z:.2f}) ts={ts}")
        else:
            # send honest update
            send = {
                "id": MY_ID,
                "x": x,
                "y": y,
                "z": z,
                "rx": rx,
                "ry": ry,
                "rz": rz,
                "vx": vx,
                "vy": vy,
                "vz": vz,
                "ts": ts,
            }
            print(f"[SEND OK] pos=({x:.2f},{y:.2f},{z:.2f}) ts={ts}")

        sock.sendto(json.dumps(send).encode('utf-8'), (SERVER_IP, SERVER_PORT))

        # wait briefly and check for server messages
        sock.settimeout(0.25)
        try:
            resp, _ = sock.recvfrom(4096)
            obj = json.loads(resp.decode('utf-8'))
            if isinstance(obj, dict) and obj.get('action') == 'correction':
                corr = obj.get('corrected')
                if isinstance(corr, dict) and corr.get('id') == MY_ID:
                    # apply correction to local true state
                    x = float(corr.get('x', x))
                    y = float(corr.get('y', y))
                    z = float(corr.get('z', z))
                    vx = float(corr.get('vx', vx))
                    vy = float(corr.get('vy', vy))
                    vz = float(corr.get('vz', vz))
                    print(f"[CORRECTED] new_pos=({x:.2f},{y:.2f},{z:.2f}) new_v=({vx:.3f},{vy:.3f},{vz:.3f})")
            else:
                # could be world state broadcast; print short summary
                if isinstance(obj, dict) and 'players' in obj:
                    players = obj.get('players', {})
                    if MY_ID in players:
                        p = players[MY_ID]
                        print(f"[WORLD] server_pos=({p.get('x')},{p.get('y')},{p.get('z')})")
        except socket.timeout:
            pass

        time.sleep(0.5)
except KeyboardInterrupt:
    print("退出 evil client")
