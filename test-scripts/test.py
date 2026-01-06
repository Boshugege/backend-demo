import socket
import time
import json
import random
import sys

# 配置
# server
SERVER_IP = "127.0.0.1"
SERVER_PORT = 8888

sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)

# ask user for a custom username before registering
MY_NAME = input("请输入用户名: ").strip()
if MY_NAME == "":
    MY_NAME = "player_" + str(random.randint(1, 1000))

print(f"尝试注册用户名: {MY_NAME}")

# register and obtain uuid
sock.settimeout(2.0)
reg = {"type": "register", "username": MY_NAME}
sock.sendto(json.dumps(reg).encode('utf-8'), (SERVER_IP, SERVER_PORT))
try:
    resp, _ = sock.recvfrom(4096)
    r = json.loads(resp.decode('utf-8'))
    if isinstance(r, dict) and r.get('action') == 'name_conflict':
        suggested = r.get('suggested')
        print(f"服务器建议更名为 {suggested}")
        MY_NAME = input(f"请输入新用户名（回车接收建议 {suggested}）: ").strip() or suggested
        reg = {"type": "register", "username": MY_NAME}
        sock.sendto(json.dumps(reg).encode('utf-8'), (SERVER_IP, SERVER_PORT))
        resp, _ = sock.recvfrom(4096)
        r = json.loads(resp.decode('utf-8'))

    if isinstance(r, dict) and r.get('action') == 'registered':
        UUID = r.get('uuid')
        print(f"已注册，uuid={UUID}")
    else:
        print("注册失败，继续使用临时用户名")
        UUID = None
except socket.timeout:
    print("注册无响应，继续使用临时用户名")
    UUID = None

print(f"我是 {MY_NAME}, 开始发送三维位移/旋转/速度数据...")
# 本地维护最近一次世界状态
latest_world = {"players": {}}

try:
    # 3D position
    x, y, z = 0.0, 0.0, 0.0
    # rotation (Euler)
    rx, ry, rz = 0.0, 0.0, 0.0
    # velocities
    vx, vy, vz = 0.1, 0.0, 0.0

    last_heartbeat = time.time()
    while True:
        # 简单物理积分：位置 += 速度
        x += vx
        y += vy
        z += vz

        # 随机扰动速度与朝向，模拟控制输入
        if random.random() < 0.2:
            vx += random.uniform(-0.05, 0.05)
            vy += random.uniform(-0.05, 0.05)
            vz += random.uniform(-0.05, 0.05)
            rx += random.uniform(-1.0, 1.0)
            ry += random.uniform(-1.0, 1.0)
            rz += random.uniform(-1.0, 1.0)

        ts = int(time.time() * 1000)
        payload = {
            "type": "update",
            "uuid": UUID,
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

        sock.sendto(json.dumps(payload).encode('utf-8'), (SERVER_IP, SERVER_PORT))

        # 接收服务器回传的世界状态或控制消息
        sock.settimeout(0.1)
        try:
            response, _ = sock.recvfrom(4096)
            payload = json.loads(response.decode('utf-8'))
            # 名称冲突提示
            if isinstance(payload, dict) and payload.get('action') == 'name_conflict':
                suggested = payload.get('suggested')
                if suggested:
                    print(f"服务器: 名称冲突，建议使用 {suggested}")
                    MY_ID = suggested
            # 世界状态
            elif isinstance(payload, dict) and 'players' in payload:
                latest_world = payload
                players = latest_world.get('players', {})
                print(f"服务器返回世界状态: {len(players)} 个玩家在线")
                # 打印每个玩家的 transform/velocity 简短信息
                for pid, p in players.items():
                    print(pid, {k: p.get(k) for k in ['x','y','z','rx','ry','rz','vx','vy','vz']})
            # 服务器要求纠正客户端位置
            elif isinstance(payload, dict) and payload.get('action') == 'correction':
                corr = payload.get('corrected')
                if isinstance(corr, dict) and corr.get('id') == MY_ID:
                    # apply correction
                    x = corr.get('x', x)
                    y = corr.get('y', y)
                    z = corr.get('z', z)
                    vx = corr.get('vx', vx)
                    vy = corr.get('vy', vy)
                    vz = corr.get('vz', vz)
                    print(f"收到纠正：位置 -> ({x},{y},{z}), 速度 -> ({vx},{vy},{vz})")
            else:
                print("收到未知消息:", payload)
        except socket.timeout:
            pass

        # heartbeat every 30s
        now = time.time()
        if UUID and now - last_heartbeat > 30:
            hb = {"type": "heartbeat", "uuid": UUID}
            try:
                sock.sendto(json.dumps(hb).encode('utf-8'), (SERVER_IP, SERVER_PORT))
            except Exception:
                pass
            last_heartbeat = now

        time.sleep(0.05)
except KeyboardInterrupt:
    print("退出")