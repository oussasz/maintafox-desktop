#!/usr/bin/env python3
"""Fix RAMS KPI eligibility: ensure demo failure events with eligible_unplanned_mtbf flag."""
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
EQ_IDS = [int(x) for x in eq_args] if eq_args else None

ELIGIBLE_FLAGS = json.dumps({
    "eligible_unplanned_mtbf": True,
    "eligible_for_strict_mtbf": True,
    "ot_linked": True,
    "rams_demo_seed": True,
})

now = datetime.now(timezone.utc)
now_s = now.strftime("%Y-%m-%dT%H:%M:%SZ")

con = sqlite3.connect(DB)
cur = con.cursor()

if EQ_IDS is None:
    rows = cur.execute(
        "SELECT DISTINCT equipment_id FROM equipment e "
        "WHERE deleted_at IS NULL AND EXISTS (SELECT 1 FROM reliability_kpi_snapshots k WHERE k.equipment_id = e.id)"
    ).fetchall()
    EQ_IDS = [r[0] for r in rows]

for eq_id in EQ_IDS:
    # Upgrade injector events missing eligibility (licensed mode keeps rows but KPI ignores them)
    cur.execute(
        "UPDATE failure_events SET eligible_flags_json = ?, updated_at = ? "
        "WHERE equipment_id = ? AND source_type = 'work_order' "
        "AND eligible_flags_json NOT LIKE '%eligible_unplanned_mtbf%true%'",
        (ELIGIBLE_FLAGS, now_s, eq_id),
    )
    upgraded = cur.rowcount

    # Ensure at least 8 demo failure events with eligibility
    existing_demo = cur.execute(
        "SELECT COUNT(*) FROM failure_events WHERE equipment_id = ? AND source_type = 'rams_demo_seed'",
        (eq_id,),
    ).fetchone()[0]

    if existing_demo < 8:
        for i in range(8):
            entity_sync_id = f"RAMS-DEMO-FE-{eq_id}-{i}"
            if cur.execute(
                "SELECT 1 FROM failure_events WHERE entity_sync_id = ?", (entity_sync_id,)
            ).fetchone():
                continue
            months_ago = 8 - i
            failed_at = (now - timedelta(days=months_ago * 30 + 5)).strftime("%Y-%m-%dT%H:%M:%SZ")
            restored_at = (now - timedelta(days=months_ago * 30 + 4)).strftime("%Y-%m-%dT%H:%M:%SZ")
            source_id = eq_id * 100 + i
            cur.execute(
                "INSERT INTO failure_events (entity_sync_id, source_type, source_id, equipment_id, "
                "detected_at, failed_at, restored_at, downtime_duration_hours, active_repair_hours, "
                "waiting_hours, is_planned, cause_not_determined, verification_status, "
                "eligible_flags_json, row_version, created_at, updated_at) "
                "VALUES (?, 'rams_demo_seed', ?, ?, ?, ?, ?, 4.0, 3.0, 1.0, 0, 0, 'verified', ?, 1, ?, ?)",
                (entity_sync_id, source_id, eq_id, failed_at, failed_at, restored_at, ELIGIBLE_FLAGS, now_s, now_s),
            )
        print(f"equipment {eq_id}: inserted demo failure events")
    else:
        cur.execute(
            "UPDATE failure_events SET eligible_flags_json = ?, verification_status = 'verified', updated_at = ? "
            "WHERE equipment_id = ? AND source_type = 'rams_demo_seed'",
            (ELIGIBLE_FLAGS, now_s, eq_id),
        )

    print(f"equipment {eq_id}: upgraded {upgraded} injector events, demo_fe={existing_demo}")

con.commit()
con.close()
print("Done. Restart app or click Refresh on RAMS dashboard to recompute KPI snapshots.")
