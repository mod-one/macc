#!/usr/bin/env python3
"""Opt-in real Chromium layout/draft checks; only a local fixture is served.
No browser is downloaded and no external provider or MACC project is contacted.
"""
import argparse
import html
import json
import os
from pathlib import Path
import re
import shutil
import signal
import socket
import subprocess
import tempfile
import time
import urllib.request

SOURCE=Path(__file__).resolve().parents[2]
def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--browser',default=os.environ.get('MACC_CHROMIUM_BIN'))
    parser.add_argument('--browser-arg',action='append',default=[])
    parser.add_argument('--report',type=Path,required=True)
    args=parser.parse_args()
    browser=args.browser or next((shutil.which(v) for v in ['chromium','chromium-browser','google-chrome'] if shutil.which(v)),None)
    if not browser:
        args.report.write_text(json.dumps({'status':'not_run','reason':'No Chromium executable supplied; no visual qualification claimed'},indent=2)+'\n')
        return 77
    with socket.socket() as sock:
        sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
    url=f'http://127.0.0.1:{port}/tests/decision-browser.html'
    with tempfile.TemporaryDirectory(prefix='macc-decision-browser-') as directory:
        with open(Path(directory)/'vite.log','w+') as log:
            server=subprocess.Popen(['node',str(SOURCE/'web/node_modules/vite/bin/vite.js'),'--host','127.0.0.1','--port',str(port),'--strictPort'],cwd=SOURCE/'web',stdout=log,stderr=log,start_new_session=True)
            report={'status':'failed','fixture':'local DecisionCenter component, mocked network','checks':[]}
            try:
                for _ in range(100):
                    try:
                        with urllib.request.urlopen(url,timeout=.2):break
                    except OSError:time.sleep(.05)
                else:raise RuntimeError('Local fixture server did not start')
                for width,height in [(320,640),(768,1024),(1440,900)]:
                    result=subprocess.run([browser,'--headless=new','--disable-gpu','--disable-dev-shm-usage',f'--window-size={width},{height}','--virtual-time-budget=10000',f'--user-data-dir={directory}/profile-{width}',*args.browser_arg,'--dump-dom',url],capture_output=True,text=True,timeout=45)
                    match=re.search(r'<pre id="qualification-result">(.*?)</pre>',result.stdout,re.S)
                    check=json.loads(html.unescape(match.group(1))) if match else {'status':'failed','error':'No browser result'}
                    report['checks'].append(check)
                    if result.returncode or check.get('status')!='passed':raise RuntimeError(f'Viewport {width}x{height}: {check}')
                report['status']='passed'
            except Exception as error:report['error']=str(error)
            finally:
                if server.poll() is None:os.killpg(server.pid,signal.SIGTERM)
                try:server.wait(timeout=5)
                except subprocess.TimeoutExpired:os.killpg(server.pid,signal.SIGKILL);server.wait(timeout=5)
                args.report.write_text(json.dumps(report,indent=2)+'\n')
            return 0 if report['status']=='passed' else 1
if __name__=='__main__':raise SystemExit(main())
