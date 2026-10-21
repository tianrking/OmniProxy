"""Exercise an extracted Debian package without installing it system-wide."""
import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time
import urllib.request
import xml.etree.ElementTree as ET


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("package", type=Path)
    args = parser.parse_args()
    if os.name != "posix":
        raise SystemExit("Debian package acceptance requires Linux")
    origin = "http://127.0.0.1:19093"
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    def request(path, data=None):
        encoded = None if data is None else json.dumps(data).encode()
        req = urllib.request.Request(origin + path, data=encoded,
                                     headers={"Content-Type": "application/json"})
        with opener.open(req, timeout=15) as response:
            return json.load(response)

    with tempfile.TemporaryDirectory(prefix="omni-deb-acceptance-") as temporary:
        root = Path(temporary)
        subprocess.run(["dpkg-deb", "-x", str(args.package.resolve()), str(root / "package")], check=True)
        binaries = root / "package/usr/bin"
        with (root / "server.log").open("w+") as log:
            server = subprocess.Popen([str(binaries / "omni-workbench"), "--listen",
                                       "127.0.0.1:19093", "--workspace", str(root / "workspace")],
                                      stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            try:
                for _ in range(100):
                    if server.poll() is not None:
                        raise RuntimeError("packaged workbench exited during startup")
                    try:
                        request("/api/status")
                        break
                    except OSError:
                        time.sleep(0.1)
                else:
                    raise RuntimeError("packaged workbench did not become ready")
                recording = request("/api/recording/start", {"port": 19094})
                assert request("/api/recording/demo", {})["recorded"] == 3
                for _ in range(30):
                    capture = request("/api/captures/" + recording["id"])
                    if len(capture["requests"]) == 3 and not capture["issues"]:
                        break
                    time.sleep(0.1)
                else:
                    raise RuntimeError("recording did not flush all three responses")
                request("/api/recording/stop", {})
                capture = request("/api/captures/" + recording["id"])
                assert len(capture["requests"]) == 3 and not capture["issues"], capture
                assert all(r["replayable"] and r["response"] and not r["response"]["issues"]
                           for r in capture["requests"]), capture
                scenario = request("/api/captures/" + recording["id"] + "/scenario",
                                   {"indexes": [r["index"] for r in capture["requests"]], "name": "Debian package acceptance"})
                assert request("/api/scenarios/" + scenario["id"] + "/run", {})["status"] == "passed"
                source = root / "scenario.json"
                source.write_text(json.dumps(scenario), encoding="utf-8")
                fault = root / "fault.json"
                fault.write_text(json.dumps({"path": "/lab/api/orders", "kind": "status", "status": 503}), encoding="utf-8")
                cli = [str(binaries / "omni-scenario"), "run", str(source)]
                normal = subprocess.run(cli, capture_output=True, text=True, timeout=30)
                assert normal.returncode == 0, normal.stderr
                failed = subprocess.run(cli + ["--fault", str(fault), "--report", str(root / "failed.json"),
                                               "--junit", str(root / "failed.xml")], capture_output=True, text=True, timeout=30)
                assert failed.returncode != 0
                assert json.loads((root / "failed.json").read_text())["status"] == "failed"
                assert ET.parse(root / "failed.xml").getroot().attrib["failures"] == "1"
                print("Debian package acceptance passed: core lookup, real recording, inferred replay, CLI and JUnit")
            finally:
                try:
                    request("/api/recording/stop", {})
                except OSError:
                    pass
                if server.poll() is None:
                    os.killpg(server.pid, signal.SIGTERM)
                    server.wait(timeout=10)


if __name__ == "__main__":
    main()
