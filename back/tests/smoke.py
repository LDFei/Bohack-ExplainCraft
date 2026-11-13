"""后端联调冒烟测试：需要先启动后端（默认 http://127.0.0.1:8787）。

用法：python back/tests/smoke.py [base_url]
覆盖文档第 9 节的主要验收场景（演示供应商）。
"""

import json
import pathlib
import sys
import time
import urllib.error
import urllib.request
import uuid

sys.stdout.reconfigure(encoding="utf-8")
BASE = (sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:8787").rstrip("/")
API = BASE + "/api/v1"
OPENER = urllib.request.build_opener(urllib.request.ProxyHandler({}))
FAILURES = []


def call(method, path, body=None, key=None, headers=None, token=None, raw=False):
    data = None
    hdrs = dict(headers or {})
    if isinstance(body, (bytes, bytearray)):
        data = bytes(body)
    elif body is not None:
        data = json.dumps(body).encode()
        hdrs["Content-Type"] = "application/json"
    if key:
        hdrs["Idempotency-Key"] = key
    if token:
        hdrs["Authorization"] = "Bearer " + token
    url = path if path.startswith("http") else API + path
    request = urllib.request.Request(url, data=data, method=method, headers=hdrs)
    try:
        with OPENER.open(request, timeout=30) as response:
            payload = response.read()
            status = response.status
    except urllib.error.HTTPError as error:
        payload = error.read()
        status = error.code
    if raw:
        return status, payload
    return status, (json.loads(payload) if payload else None)


def check(name, condition, detail=""):
    print(("  ok   " if condition else "  FAIL ") + name + (f"  ({detail})" if detail and not condition else ""))
    if not condition:
        FAILURES.append(name)


def new_key():
    return uuid.uuid4().hex


def wait_job(job_id, statuses=("succeeded", "failed", "cancelled", "waiting_input"), timeout=60):
    deadline = time.time() + timeout
    while time.time() < deadline:
        _, body = call("GET", f"/jobs/{job_id}")
        job = body["data"]["job"]
        if job["status"] in statuses:
            return job
        time.sleep(0.5)
    raise TimeoutError(job_id)


def snapshot(project_id):
    return call("GET", f"/projects/{project_id}")[1]["data"]


def send(project_id, text, intent="generate", scenario=None, assets=None):
    snap = snapshot(project_id)
    body = {
        "intent": intent,
        "text": text,
        "asset_ids": assets or [],
        "expected_draft_revision": snap["project"]["draft_revision"],
        "base_video_version_id": snap["project"]["latest_video_version_id"] if intent == "revise" else None,
    }
    headers = {"X-Demo-Scenario": scenario} if scenario else None
    return call("POST", f"/projects/{project_id}/messages", body, key=new_key(), headers=headers)


def new_project():
    status, body = call("POST", "/projects", {}, key=new_key())
    assert status == 201, body
    return body["data"]["project"]["id"]


print("1. 一句话生成")
pid = new_project()
status, body = send(pid, "帮我做一个30秒的咖啡店宣传视频，温暖治愈一点。")
check("消息受理 202", status == 202, body)
job_id = body["data"]["job_id"]
status, busy = send(pid, "再来一个")
check("制作中再次发送返回 PROJECT_BUSY", busy.get("error", {}).get("code") in ("PROJECT_BUSY", "REVISION_CONFLICT"), busy)
job = wait_job(job_id)
check("任务成功", job["status"] == "succeeded", job.get("error"))
snap = snapshot(pid)
check("项目 ready 且有成片", snap["project"]["status"] == "ready" and snap["latest_video"] is not None)
check("标题自动生成", snap["project"]["title"] == "咖啡店的温柔早晨", snap["project"]["title"])
check("草稿无待更新阶段", snap["draft"]["invalidated_stages"] == [] and not snap["project"]["draft_dirty"])
check("分镜有缩略图", all(s.get("thumbnail_url") for s in snap["draft"]["shots"]))
assistant = [m for m in snap["messages_page"]["items"] if m["role"] == "assistant"][-1]
check("assistant 消息带 plan/progress/video 卡片", [b["type"] for b in assistant["blocks"]] == ["plan", "progress", "video"], assistant["blocks"])
video = snap["latest_video"]
status, data = call("GET", BASE + video["playback_url"], raw=True)
check("成片可播放", status == 200 and len(data) > 10000, status)

print("2. 事件流补发")
status, raw = call("GET", f"/projects/{pid}/events", raw=True)
check("缺少游标返回 400", status == 400)
status, raw = call("GET", f"/projects/{pid}/events?after=999999", raw=True)
check("未来游标返回 400", status == 400)

print("3. 对话修改（只改配音）")
first_shots = {s["id"]: s["asset_ids"] for s in snap["draft"]["shots"]}
status, body = send(pid, "语速慢一点，换成男声", intent="revise")
check("修改受理", status == 202, body)
job = wait_job(body["data"]["job_id"])
check("修改成功", job["status"] == "succeeded", job.get("error"))
stages = {s["key"]: s["status"] for s in job["stages"]}
check("画面阶段复用（skipped）", stages["visuals"] == "skipped", stages)
check("音频阶段重新执行", stages["audio"] == "succeeded", stages)
snap2 = snapshot(pid)
check("生成 v2", snap2["latest_video"]["version_number"] == 2)
check("配音参数已更新", snap2["draft"]["audio"]["voice_id"] == "voice_calm" and snap2["draft"]["audio"]["speech_rate"] == 0.9)
check("镜头画面未变", {s["id"]: s["asset_ids"] for s in snap2["draft"]["shots"]} == first_shots)
_, versions = call("GET", f"/projects/{pid}/versions")
check("版本列表 2 条且省略快照", len(versions["data"]["items"]) == 2 and "snapshot" not in versions["data"]["items"][0])
v1 = versions["data"]["items"][1]["id"]
_, v1d = call("GET", f"/projects/{pid}/versions/{v1}")
check("旧版本快照保留原配音", v1d["data"]["version"]["snapshot"]["audio"]["voice_id"] == "voice_warm")

print("4. 只换第二个镜头")
status, body = send(pid, "第二个镜头换成咖啡特写", intent="revise")
job = wait_job(body["data"]["job_id"])
snap3 = snapshot(pid)
changed = [s["id"] for s in snap3["draft"]["shots"] if s["asset_ids"] != first_shots.get(s["id"])]
check("只有第二个镜头换了画面", changed == [snap3["draft"]["shots"][1]["id"]], changed)

print("5. 手动保存设置后更新")
rev = snap3["project"]["draft_revision"]
finishing = dict(snap3["draft"]["finishing"], color_preset_id="warm", effect_preset_id="fade")
status, body = call("PATCH", f"/projects/{pid}/draft", {"expected_draft_revision": rev, "changes": {"finishing": finishing}})
check("保存草稿 200", status == 200, body)
check("调色只让 finishing/render 失效", body["data"]["draft"]["invalidated_stages"] == ["finishing", "render"], body["data"]["draft"]["invalidated_stages"])
status, body = call("PATCH", f"/projects/{pid}/draft", {"expected_draft_revision": rev, "changes": {"finishing": finishing}})
check("旧版本号保存返回 REVISION_CONFLICT", body["error"]["code"] == "REVISION_CONFLICT")
status, body = call("PATCH", f"/projects/{pid}/draft", {"expected_draft_revision": rev + 1, "changes": {"brief": None}})
check("不允许修改 brief（422）", status == 422, status)
status, body = send(pid, "按当前已保存的脚本与设置更新视频。", intent="revise")
job = wait_job(body["data"]["job_id"])
check("更新视频成功", job["status"] == "succeeded")
check("v4 快照带新调色", snapshot(pid)["latest_video"]["snapshot"]["finishing"]["color_preset_id"] == "warm")

print("6. 失败与重试")
pid2 = new_project()
status, body = send(pid2, "做一个山野旅行短片", scenario="failure")
job = wait_job(body["data"]["job_id"])
check("画面阶段失败", job["status"] == "failed" and job["error"]["failed_stage"] == "visuals", job.get("error"))
snapf = snapshot(pid2)
check("项目 failed，保留脚本分镜", snapf["project"]["status"] == "failed" and len(snapf["draft"]["shots"]) == 4)
rev = snapf["project"]["draft_revision"]
status, body = call("POST", f"/jobs/{job['id']}/retry", {"expected_draft_revision": rev}, key=new_key())
check("重试受理", status == 202 and body["data"]["parent_job_id"] == job["id"], body)
retry = wait_job(body["data"]["job_id"])
stages = {s["key"]: s["status"] for s in retry["stages"]}
check("重试从画面继续（文案分镜复用）", retry["status"] == "succeeded" and stages["script"] == "skipped" and stages["storyboard"] == "skipped", stages)
status, body = call("POST", f"/jobs/{job['id']}/retry", {"expected_draft_revision": snapshot(pid2)["project"]["draft_revision"]}, key=new_key())
check("草稿变化后旧任务重试被拒", body["error"]["code"] == "RETRY_INPUT_CHANGED", body)

print("7. 追问")
pid3 = new_project()
status, body = send(pid3, "帮我做一个温暖的咖啡店宣传视频。", scenario="question")
job = wait_job(body["data"]["job_id"])
check("进入 waiting_input", job["status"] == "waiting_input" and job["question"], job["status"])
check("项目 waiting_input", snapshot(pid3)["project"]["status"] == "waiting_input")
status, body = call("POST", f"/jobs/{job['id']}/answers", {"question_id": "question_wrong", "value": "x"}, key=new_key())
check("错误问题 ID 返回 QUESTION_NOT_ACTIVE", body["error"]["code"] == "QUESTION_NOT_ACTIVE")
key = new_key()
status, body = call("POST", f"/jobs/{job['id']}/answers", {"question_id": job["question"]["id"], "value": "暖日咖啡"}, key=key)
check("回答受理", status == 202, body)
status, again = call("POST", f"/jobs/{job['id']}/answers", {"question_id": job["question"]["id"], "value": "暖日咖啡"}, key=key)
check("同 key 重发不重复执行", status == 202 and again["data"] == body["data"])
job = wait_job(job["id"], ("succeeded", "failed", "cancelled"))
check("回答后完成同一任务", job["status"] == "succeeded")
snapq = snapshot(pid3)
check("店名写进脚本", "暖日咖啡" in snapq["draft"]["script"]["text"])

print("8. 取消")
pid4 = new_project()
status, body = send(pid4, "做一个产品介绍视频，随行水杯")
time.sleep(1.5)
status, cancel = call("POST", f"/jobs/{body['data']['job_id']}/cancel")
check("取消返回 202 cancelling", status == 202 and cancel["data"]["job_status"] == "cancelling", cancel)
job = wait_job(body["data"]["job_id"], ("cancelled", "succeeded", "failed"))
check("任务最终 cancelled", job["status"] == "cancelled", job["status"])
status, again = call("POST", f"/jobs/{job['id']}/cancel")
check("重复取消返回 200", status == 200)
check("项目释放", snapshot(pid4)["project"]["active_job_id"] is None)
status, body = call("POST", f"/jobs/{job_id}/cancel")
check("取消已完成任务返回 JOB_ALREADY_FINISHED", body["error"]["code"] == "JOB_ALREADY_FINISHED")

print("9. 仅讨论")
status, body = send(pid, "你好，怎么用？", intent="chat")
job = wait_job(body["data"]["job_id"])
snapc = snapshot(pid)
reply = [m for m in snapc["messages_page"]["items"] if m["id"] == job["result"]["assistant_message_id"]][0]
check("聊天回答完成且不生成新版本", job["status"] == "succeeded" and reply["text"] and snapc["latest_video"]["version_number"] == 4)
check("流式增量序号已记录", reply["last_delta_seq"] >= 1)

print("10. 上传素材")
jpeg = (pathlib.Path(__file__).resolve().parents[2] / "front" / "public" / "media" / "coffee-cup.jpg").read_bytes()
boundary = "----smoke" + uuid.uuid4().hex
multipart = (
    f"--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"cup.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n".encode()
    + jpeg
    + f"\r\n--{boundary}--\r\n".encode()
)
pid5 = new_project()
status, body = call("POST", f"/projects/{pid5}/assets", multipart, key=new_key(), headers={"Content-Type": f"multipart/form-data; boundary={boundary}"})
check("上传图片 201 且解析尺寸", status == 201 and body["data"]["asset"]["width"], body)
asset_id = body["data"]["asset"]["id"]
bad = (
    f"--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.jpg\"\r\n\r\nnot an image\r\n--{boundary}--\r\n".encode()
)
status, body = call("POST", f"/projects/{pid5}/assets", bad, key=new_key(), headers={"Content-Type": f"multipart/form-data; boundary={boundary}"})
check("伪装扩展名被拒（415）", status == 415, status)
status, body = send(pid5, "用我的照片做一个咖啡店视频", assets=[asset_id])
job = wait_job(body["data"]["job_id"])
snapu = snapshot(pid5)
check("上传素材放进第一个镜头", job["status"] == "succeeded" and snapu["draft"]["shots"][0]["asset_ids"] == [asset_id])
_, assets = call("GET", f"/projects/{pid5}/assets")
check("素材列表只含上传文件", [a["id"] for a in assets["data"]["items"]] == [asset_id])

print("11. 导出与下载")
ver = snapshot(pid)["latest_video"]
status, body = call("POST", f"/projects/{pid}/versions/{ver['id']}/exports", {"format": "mp4", "resolution": "720p", "fps": 30}, key=new_key())
check("720p 导出复用预览文件", status == 200 and body["data"]["cached"], body)
status, body = call("POST", f"/projects/{pid}/versions/{ver['id']}/exports", {"format": "mp4", "resolution": "1080p", "fps": 30}, key=new_key())
check("未接通的规格返回 422", status == 422)
status, body = call("GET", f"/assets/{ver['video_asset_id']}/download")
check("下载链接带文件名", status == 200 and body["data"]["file_name"].endswith("-v4.mp4"), body)
status, raw = call("GET", BASE + body["data"]["download_url"], raw=True)
check("下载可用", status == 200 and len(raw) > 10000)

print("12. 隔离与删除")
status, body = call("GET", f"/projects/{pid}", token="another-user")
check("其他用户访问返回 404", status == 404)
status, body = call("DELETE", f"/projects/{pid4}")
check("删除项目 204", status == 204)
check("删除后 404", call("GET", f"/projects/{pid4}")[0] == 404)

print()
print("全部通过" if not FAILURES else f"{len(FAILURES)} 项失败：{FAILURES}")
sys.exit(1 if FAILURES else 0)
