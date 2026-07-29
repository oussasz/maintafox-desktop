#!/usr/bin/env python3
"""Recompute 12-month reliability_kpi_snapshots after eligibility fix (approximates Rust compute.rs)."""
import json
import sqlite3
import sys
from datetime import datetime, timedelta, timezone

DEFAULT_DB = r"C:\Users\LENOVO\AppData\Roaming\systems.maintafox.desktop\maintafox.db"
args = sys.argv[1:]
if args and args[0].lower().endswith(".db"):
    DB = args[0]
    eq_args = args[1:]
else:
    DB = DEFAULT_DB
    eq_args = args

now = datetime.now(timezone.utc)
period_end = now
period_start = now - timedelta(days=365)
p0s = period_start.strftime("%Y-%m-%dT%H:%M:%SZ")
p1s = period_end.strftime("%Y-%m-%dT%H:%M:%SZ")
min_n = 5


def eligible(flags_json: str) -> bool:
    try:
        v = json.loads(flags_json or "{}")
        x = v.get("eligible_unplanned_mtbf")
        return x is True or x == 1
    except json.JSONDecodeError:
        return False


def parse_dt(s: str) -> datetime:
    return datetime.fromisoformat(s.replace("Z", "+00:00"))


con = sqlite3.connect(DB)
cur = con.cursor()

if eq_args:
    eq_ids = [int(x) for x in eq_args]
else:
    eq_ids = [
        r[0]
        for r in cur.execute(
            "SELECT id FROM equipment WHERE deleted_at IS NULL ORDER BY id"
        ).fetchall()
    ]

for eq_id in eq_ids:
    t_exp = 1888.0  # matches calendar_operating_schedule_inferred demo window
    rows = cur.execute(
        "SELECT id, eligible_flags_json, downtime_duration_hours, active_repair_hours, "
        "COALESCE(failed_at, detected_at, created_at) AS ev_ts "
        "FROM failure_events WHERE equipment_id = ?",
        (eq_id,),
    ).fetchall()

    in_period = []
    for row in rows:
        _id, flags, downtime, active_r, ev_ts = row
        if not eligible(flags):
            continue
        ts = parse_dt(ev_ts)
        if period_start <= ts <= period_end:
            in_period.append((downtime or 0.0, active_r or 0.0))

    f = len(in_period)
    d_down = sum(x[0] for x in in_period)
    repairs = [x[1] for x in in_period if (x[1] or 0) > 0]
    mtbf = (t_exp / f) if f > 0 and t_exp > 0 else None
    mttr = (sum(repairs) / len(repairs)) if repairs else None
    failure_rate = (f / t_exp) if t_exp > 0 else None
    availability = max(0.0, t_exp - d_down) / t_exp if t_exp > 0 else None
    dq = 1.0 if f >= min_n else 0.49 * min(1.0, f / min_n)

    existing = cur.execute(
        "SELECT id FROM reliability_kpi_snapshots WHERE equipment_id=? AND period_start=? AND period_end=?",
        (eq_id, p0s, p1s),
    ).fetchone()

    vals = (mtbf, mttr, availability, failure_rate, f, dq)
    if existing:
        cur.execute(
            "UPDATE reliability_kpi_snapshots SET mtbf=?, mttr=?, availability=?, failure_rate=?, "
            "event_count=?, data_quality_score=?, row_version=row_version+1 WHERE id=?",
            (*vals, existing[0]),
        )
        print(f"equipment {eq_id}: updated KPI snapshot id={existing[0]} events={f} mtbf={mtbf}")
    else:
        eid = f"kpi_snapshot:demo-{eq_id}"
        cur.execute(
            "INSERT INTO reliability_kpi_snapshots (entity_sync_id, equipment_id, period_start, period_end, "
            "mtbf, mttr, availability, failure_rate, repeat_failure_rate, event_count, data_quality_score, row_version) "
            "VALUES (?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, ?, 1)",
            (eid, eq_id, p0s, p1s, *vals),
        )
        print(f"equipment {eq_id}: inserted KPI snapshot events={f} mtbf={mtbf}")

con.commit()
con.close()
print("KPI snapshots refreshed.")
