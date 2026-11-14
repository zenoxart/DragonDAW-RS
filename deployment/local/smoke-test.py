#!/usr/bin/env python3
"""End-to-end smoke test of a running Red Hive backend (Docker or local).

    python3 deployment/local/smoke-test.py [http://localhost:8080]

Covers: sign-in, permissions, live notifications (SSE), invoice approval with the AI agent and an approval
inbox task, limit/self-approval rules, reports, user creation, module publishing with impact analysis
(auto-migrated / needs review / broken) and the held rollout.
"""

import json
import sys
import threading
import time
import urllib.error
import urllib.request
from datetime import date, timedelta
from pathlib import Path

BASE = (sys.argv[1] if len(sys.argv) > 1 else "http://localhost:8080").rstrip("/")
PASSWORD = "RedHive!2026"
ROOT = Path(__file__).resolve().parents[2]
failures = []


def call(method, path, token=None, body=None, expect=None):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(BASE + path, data=data, method=method)
    req.add_header("content-type", "application/json")
    if token:
        req.add_header("authorization", "Bearer " + token)
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            raw = r.read()
            status = r.status
    except urllib.error.HTTPError as e:
        raw = e.read()
        status = e.code
    payload = json.loads(raw) if raw else None
    if expect is not None and status != expect:
        raise AssertionError(f"{method} {path}: expected {expect}, got {status}: {payload}")
    return status, payload


def check(name, cond, detail=""):
    print(("  ok   " if cond else "  FAIL ") + name + (f" – {detail}" if detail and not cond else ""))
    if not cond:
        failures.append(name)


def login(user):
    _, r = call("POST", "/api/identity/login", body={"userName": user, "password": PASSWORD}, expect=200)
    return r["accessToken"], r["user"]


def wait_for(fn, timeout=30, interval=0.5):
    end = time.time() + timeout
    while time.time() < end:
        v = fn()
        if v:
            return v
        time.sleep(interval)
    return None


class Stream(threading.Thread):
    """Collects server-sent events of one user."""

    def __init__(self, token):
        super().__init__(daemon=True)
        self.token = token
        self.events = []

    def run(self):
        req = urllib.request.Request(BASE + "/api/notifications/stream?access_token=" + self.token)
        with urllib.request.urlopen(req, timeout=120) as r:
            data = []
            for line in r:
                line = line.decode().rstrip("\n")
                if line.startswith("data:"):
                    data.append(line[5:].strip())
                elif line == "" and data:
                    try:
                        self.events.append(json.loads("\n".join(data)))
                    except ValueError:
                        pass
                    data = []

    def types(self):
        return [e.get("type") for e in self.events if isinstance(e, dict)]


def main():
    print(f"Red Hive smoke test against {BASE}")

    print("identity")
    admin, _ = login("admin")
    clerk, clerk_user = login("lukas.berger")
    maria, maria_user = login("maria.huber")
    elena, _ = login("elena.rossi")
    status, err = call("POST", "/api/identity/login", body={"userName": "tom.fischer", "password": PASSWORD})
    check("deactivated user is refused", status == 403 and err["code"] == "identity.login.inactive")
    status, err = call("POST", "/api/identity/login", body={"userName": "admin", "password": "wrong"})
    check("wrong password is refused", status == 401)
    _, me = call("GET", "/api/identity/me", clerk, expect=200)
    check("clerk has documents.edit", "documents.edit" in me["permissions"])

    print("modules")
    _, assigned = call("GET", "/api/modules/assigned", clerk, expect=200)
    ids = sorted(m["manifest"]["moduleId"] for m in assigned)
    check("three modules assigned with signed packages", ids == ["document-management", "reporting", "user-management"] and all(m["package"] for m in assigned), ids)

    print("notifications")
    stream = Stream(maria)
    stream.start()
    time.sleep(1)

    print("invoice approval (AI pre-check + person confirms the critical step)")
    today = date.today()
    inv = {"supplier": "Smoke Test Supplies", "amount": 1234.5, "currency": "EUR", "issuedOn": today.isoformat(), "dueOn": (today + timedelta(days=30)).isoformat(), "costCenter": "CC-1000"}
    _, created = call("POST", "/api/documents/invoices", clerk, inv, expect=201)
    check("invoice created as draft", created["status"] == "draft")
    status, _ = call("POST", f"/api/documents/invoices/{created['id']}/approve", maria)
    check("a draft cannot be approved", status == 409)
    _, submitted = call("POST", f"/api/documents/invoices/{created['id']}/submit", clerk, expect=200)
    check("invoice submitted", submitted["status"] == "submitted")

    def task():
        _, items = call("GET", "/api/workflows/inbox", maria, expect=200)
        return next((i for i in items if i["vars"].get("invoiceId") == created["id"]), None)

    item = wait_for(task, 30)
    check("Maria gets an inbox task from the workflow", item is not None)
    if item:
        check("the task confirms the AI's critical step", item["task"]["confirmsAi"] and item["task"]["stepId"] == "approve", item["task"])
        _, inst = call("GET", f"/api/workflows/instances/{item['instanceId']}", maria, expect=200)
        proposed = [h for h in inst["history"] if h["action"] == "proposed"]
        check("the AI proposed approval with confidence", proposed and proposed[0]["data"]["decision"] == "approve" and proposed[0]["confidence"] >= 0.7, proposed)
        status, err = call("POST", f"/api/workflows/instances/{item['instanceId']}/complete", clerk, {"decision": "approve"})
        check("the clerk cannot complete Maria's task", status == 403)
        _, done = call("POST", f"/api/workflows/instances/{item['instanceId']}/complete", maria, {"decision": "approve", "comment": "ok"}, expect=200)
        check("workflow completed", done["state"] == "completed", done["state"])
        _, after = call("GET", f"/api/documents/invoices/{created['id']}", clerk, expect=200)
        check("invoice approved by Maria via the agent", after["status"] == "approved" and "Maria Huber" in (after["decidedByName"] or ""), after)

    print("limits")
    big = dict(inv, supplier="Big Ticket GmbH", amount=24000)
    _, big_inv = call("POST", "/api/documents/invoices", clerk, big, expect=201)
    status, err = call("POST", f"/api/documents/invoices/{big_inv['id']}/submit", clerk, expect=200)
    item = wait_for(lambda: next((i for i in call("GET", "/api/workflows/inbox", elena, expect=200)[1] if i["vars"].get("invoiceId") == big_inv["id"]), None), 30)
    check("an amount above the AI limit is handed to a person", item is not None and not item["task"]["confirmsAi"] and item["task"]["stepId"] == "review", item and item["task"])
    if item:
        status, err = call("POST", f"/api/documents/invoices/{big_inv['id']}/approve", maria, {"comment": "x"})
        check("Maria's limit (10,000) blocks 24,000", status == 403 and err["code"] == "documents.invoice.limitExceeded", err)
        _, step = call("POST", f"/api/workflows/instances/{item['instanceId']}/complete", elena,
                       {"values": {"invoiceId": big_inv["id"], "amount": 24000, "recommendation": "approve", "note": "checked by Elena"}}, expect=200)
        check("the review moves on to the critical approval", step["state"] == "waitingForApproval", step["state"])
        _, done = call("POST", f"/api/workflows/instances/{item['instanceId']}/complete", elena, {"decision": "approve"}, expect=200)
        check("Elena confirms – the step runs with her limit", done["state"] == "completed", done.get("error"))
        _, after = call("GET", f"/api/documents/invoices/{big_inv['id']}", clerk, expect=200)
        check("invoice approved by Elena", after["status"] == "approved" and "Elena Rossi" in (after["decidedByName"] or ""), after)

    print("live notifications")
    got = wait_for(lambda: "documents.invoice.submitted" in stream.types(), 10)
    check("Maria received documents.invoice.submitted over SSE", bool(got), stream.types()[:10])

    print("reports")
    _, catalog = call("GET", "/api/reports/catalog", elena, expect=200)
    check("report catalogue", any(r["id"] == "sales-by-region" for r in catalog))
    _, result = call("POST", "/api/reports/sales-by-region/run", elena, {"parameters": {"from": "2025-01-01", "to": today.isoformat()}}, expect=200)
    check("report result with rows", result["rowCount"] == 4 and result["total"] > 0, result.get("rowCount"))
    status, _ = call("POST", "/api/reports/sales-by-region/run", clerk, {"parameters": {"from": "2025-01-01", "to": today.isoformat()}})
    check("a clerk may not run reports", status == 403)

    print("users")
    name = "smoke." + str(int(time.time()))[-6:]
    _, u = call("POST", "/api/users", admin, {"displayName": "Smoke Test", "userName": name, "email": name + "@redhive.demo", "role": "Clerk"}, expect=201)
    status, _ = call("POST", "/api/identity/login", body={"userName": name, "password": u["temporaryPassword"]})
    check("new user can sign in with the temporary password", status == 200)
    status, err = call("POST", "/api/users", admin, {"displayName": "X", "userName": name, "email": "x@redhive.demo"})
    check("duplicate user name is refused", status == 409 and err["code"] == "users.userName.taken", err)

    print("module update with impact analysis")
    manifest = json.loads((ROOT / "modules/document-management/demo-updates/1.1.0/manifest.json").read_text())
    status, pub = call("POST", "/api/modules", admin, {"manifest": manifest, "channel": "stable"})
    if status == 409 and pub["code"] == "modules.version.notNewer":
        print("  (1.1.0 already published – skipping)")
    else:
        check("publish accepted", status == 200, pub)
        check("rollout held because workflows would break", pub.get("rolloutStage") == "Held" and pub.get("brokenWorkflows", 0) >= 1, pub)
        _, report = call("GET", f"/api/workflows/impact/{pub['impactReportId']}", admin, expect=200)
        classes = {w["workflowName"]: w["class"] for w in report["workflows"]}
        check("invoice approval needs review", classes.get("Invoice approval") == "reviewRequired", classes)
        check("send-to-accounting auto-migrated", classes.get("Send approved invoices to accounting") == "autoMigrated", classes)
        check("archiving is broken", classes.get("Archive a finished invoice") == "broken", classes)
        _, wfs = call("GET", "/api/workflows", admin, expect=200)
        approval = next(w for w in wfs if w["name"] == "Invoice approval")
        check("needs-review workflow has its AI trigger suspended", approval["status"] == "needsReview" and any("suspended" in t for t in approval["triggers"]), approval)
        status, err = call("POST", f"/api/updates/rollouts/{pub['rolloutId']}/advance", admin, {})
        check("a held rollout cannot advance without force", status == 409, err)

    print()
    if failures:
        print(f"{len(failures)} check(s) failed: {failures}")
        sys.exit(1)
    print("all checks passed")


if __name__ == "__main__":
    main()
