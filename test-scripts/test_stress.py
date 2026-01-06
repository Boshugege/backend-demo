#!/usr/bin/env python3
"""
High-concurrency UDP stress tester for the rust UDP server.

Usage:
  python3 test_stress.py --clients 100 --rate 2 --duration 60 --server 127.0.0.1:8888

This will spawn N processes (one per client) that each send `rate` messages/sec
for `duration` seconds. Each client also listens for world broadcasts and
measures application-layer RTT by comparing the client's sent `ts` with the
`players[my_id].ts` in the broadcast.

Outputs aggregated stats at the end: total sent, total received broadcasts,
and average latency.
"""
import argparse
import json
import multiprocessing as mp
import random
import socket
import time
from typing import Tuple


def parse_server(addr: str) -> Tuple[str, int]:
    if ":" in addr:
        host, port = addr.rsplit(":", 1)
        return host, int(port)
    return addr, 8888


def client_worker(client_index: int, server_host: str, server_port: int, rate: float, duration: int, stats: dict):
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.settimeout(0.01)
    my_id = f"stress_{client_index}_{random.randint(0,9999)}"

    # local simulated state
    x = random.random() * 5.0
    y = random.random() * 5.0
    z = random.random() * 1.0
    vx = 0.1 + random.random() * 0.2
    vy = 0.0
    vz = 0.0

    interval = 1.0 / rate if rate > 0 else 0.5
    end = time.time() + duration

    sent = 0
    recvd = 0
    latency_sum = 0.0
    errors = 0

    while time.time() < end:
        ts = int(time.time() * 1000)
        x += vx * interval
        y += vy * interval
        z += vz * interval

        payload = {
            "id": my_id,
            "x": x,
            "y": y,
            "z": z,
            "vx": vx,
            "vy": vy,
            "vz": vz,
            "ts": ts,
        }

        try:
            sock.sendto(json.dumps(payload).encode("utf-8"), (server_host, server_port))
            sent += 1
        except Exception:
            errors += 1

        # listen briefly for broadcast and compute latency when we see our id
        t0 = time.time()
        listen_deadline = t0 + 0.01
        while time.time() < listen_deadline:
            try:
                data, _ = sock.recvfrom(8192)
            except socket.timeout:
                break
            except Exception:
                errors += 1
                break

            try:
                obj = json.loads(data.decode("utf-8"))
            except Exception:
                continue

            if isinstance(obj, dict) and "players" in obj:
                players = obj.get("players", {})
                if my_id in players:
                    p = players[my_id]
                    try:
                        client_ts = int(p.get("ts", ts))
                        now_ms = int(time.time() * 1000)
                        latency = now_ms - client_ts
                        recvd += 1
                        latency_sum += latency
                    except Exception:
                        pass

        time.sleep(interval)

    # publish stats into shared dict
    stats[my_id] = {"sent": sent, "recv": recvd, "latency_sum": latency_sum, "errors": errors}


def main():
    parser = argparse.ArgumentParser(description="UDP stress tester")
    parser.add_argument("--clients", type=int, default=50, help="number of concurrent clients")
    parser.add_argument("--rate", type=float, default=2.0, help="messages per second per client")
    parser.add_argument("--duration", type=int, default=30, help="test duration seconds")
    parser.add_argument("--server", type=str, default="127.0.0.1:8888", help="server host:port")
    args = parser.parse_args()

    host, port = parse_server(args.server)

    manager = mp.Manager()
    stats = manager.dict()

    procs = []
    start = time.time()
    for i in range(args.clients):
        p = mp.Process(target=client_worker, args=(i, host, port, args.rate, args.duration, stats))
        p.start()
        procs.append(p)

    for p in procs:
        p.join()

    # aggregate results
    total_sent = 0
    total_recv = 0
    total_latency = 0.0
    total_errors = 0

    for v in stats.values():
        total_sent += int(v.get("sent", 0))
        total_recv += int(v.get("recv", 0))
        total_latency += float(v.get("latency_sum", 0.0))
        total_errors += int(v.get("errors", 0))

    avg_latency = (total_latency / total_recv) if total_recv > 0 else None

    print("--- Stress test summary ---")
    print(f"clients: {args.clients}")
    print(f"duration: {args.duration}s")
    print(f"total sent: {total_sent}")
    print(f"total recv (broadcasts seen): {total_recv}")
    print(f"total errors: {total_errors}")
    if avg_latency is not None:
        print(f"avg latency (ms): {avg_latency:.2f}")
    else:
        print("avg latency (ms): N/A (no broadcasts received)")


if __name__ == "__main__":
    main()
