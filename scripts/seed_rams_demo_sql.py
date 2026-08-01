#!/usr/bin/env python3
"""Direct SQLite RAMS demo seed — license-safe (no rams_injector)."""
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
EQ_IDS = [int(x) for x in eq_args] if eq_args else [71]

ELIGIBLE_FLAGS = json.dumps({
    "eligible_unplanned_mtbf": True,
    "eligible_for_strict_mtbf": True,
    "ot_linked": True,
    "rams_demo_seed": True,
})

now = datetime.now(timezone.utc)
now_s = now.strftime("%Y-%m-%dT%H:%M:%SZ")
year_ago = (now - timedelta(days=365)).strftime("%Y-%m-%dT%H:%M:%SZ")
anchor_closed = (now - timedelta(days=330)).strftime("%Y-%m-%dT%H:%M:%SZ")

con = sqlite3.connect(DB)
cur = con.cursor()


def scalar(sql, params=()):
    return cur.execute(sql, params).fetchone()[0]


def ensure_schedule_class():
    row = cur.execute(
        """
        SELECT rv.id
          FROM reference_values rv
          JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published'
          JOIN reference_domains d ON d.id = rs.domain_id
         WHERE UPPER(TRIM(d.code)) = 'ORG.SCHEDULE_CLASS'
           AND UPPER(TRIM(rv.code)) = 'DAY_SHIFT'
           AND rv.is_active = 1
         ORDER BY rv.id ASC
         LIMIT 1
        """
    ).fetchone()
    if row:
        return row[0]

    set_row = cur.execute(
        """
        SELECT rs.id
          FROM reference_domains d
          JOIN reference_sets rs ON rs.domain_id = d.id AND rs.status = 'published'
         WHERE UPPER(TRIM(d.code)) = 'ORG.SCHEDULE_CLASS'
         ORDER BY rs.version_no DESC
         LIMIT 1
        """
    ).fetchone()
    if not set_row:
        raise RuntimeError("ORG.SCHEDULE_CLASS published set missing")
    set_id = set_row[0]
    cur.execute(
        "INSERT INTO reference_values (set_id, parent_id, code, label, description, sort_order, "
        "color_hex, icon_name, semantic_tag, external_code, is_active, metadata_json) "
        "VALUES (?, NULL, 'DAY_SHIFT', 'Journée normale', NULL, 1, NULL, NULL, 'schedule_class', NULL, 1, "
        "'{\"shift_pattern_code\":\"DAY_SHIFT\",\"is_continuous\":false,\"nominal_hours_per_day\":8.0}')",
        (set_id,),
    )
    sc_id = cur.lastrowid
    days = [
        (1, "08:00", "16:00", 0),
        (2, "08:00", "16:00", 0),
        (3, "08:00", "16:00", 0),
        (4, "08:00", "16:00", 0),
        (5, "08:00", "16:00", 0),
        (6, "08:00", "16:00", 1),
        (7, "08:00", "16:00", 1),
    ]
    for dow, start, end, rest in days:
        cur.execute(
            "INSERT OR IGNORE INTO schedule_details (reference_value_id, day_of_week, shift_start, shift_end, is_rest_day) "
            "VALUES (?, ?, ?, ?, ?)",
            (sc_id, dow, start, end, rest),
        )
    print(f"Created ORG.SCHEDULE_CLASS DAY_SHIFT id={sc_id}")
    return sc_id


def ref_id(table, where_col, val):
    row = cur.execute(f"SELECT id FROM {table} WHERE {where_col}=? LIMIT 1", (val,)).fetchone()
    if not row:
        raise RuntimeError(f"Missing {table}.{where_col}={val}")
    return row[0]


def ensure_equipment(eq_id, sc_id):
    row = cur.execute(
        "SELECT installed_at_node_id FROM equipment WHERE id=? AND deleted_at IS NULL",
        (eq_id,),
    ).fetchone()
    if not row:
        print(f"Skip equipment {eq_id}: not found")
        return
    node_id = row[0]
    if node_id is None:
        print(f"Skip equipment {eq_id}: no installed_at_node_id")
        return

    cur.execute(
        "UPDATE equipment SET rams_schedule_reference_value_id=?, rams_utilization_factor=1.0, updated_at=? "
        "WHERE id=?",
        (sc_id, now_s, eq_id),
    )

    actor_id = scalar(
        "SELECT id FROM user_accounts WHERE is_active=1 ORDER BY is_admin DESC, id ASC LIMIT 1"
    )
    closed_status = ref_id("work_order_statuses", "code", "closed")
    corrective_type = ref_id("work_order_types", "code", "corrective")
    urgency_id = ref_id("urgency_levels", "level", 3)

    code = f"RAMS-DEMO-ANCHOR-{eq_id}"
    row = cur.execute("SELECT id FROM work_orders WHERE code=?", (code,)).fetchone()
    if row:
        wo_id = row[0]
        cur.execute(
            "UPDATE work_orders SET status_id=?, closed_at=?, actual_start=?, actual_end=?, "
            "equipment_id=?, entity_id=?, updated_at=? WHERE id=?",
            (closed_status, anchor_closed, anchor_closed, anchor_closed, eq_id, node_id, now_s, wo_id),
        )
    else:
        cur.execute(
            "INSERT INTO work_orders (code, type_id, status_id, equipment_id, entity_id, requester_id, "
            "planner_id, urgency_id, title, description, actual_start, actual_end, closed_at, "
            "row_version, created_at, updated_at, closeout_validation_passed) "
            "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, 1)",
            (
                code,
                corrective_type,
                closed_status,
                eq_id,
                node_id,
                actor_id,
                actor_id,
                urgency_id,
                f"RAMS demo anchor WO for equipment {eq_id}",
                "SQL demo seed — governed exposure anchor.",
                anchor_closed,
                anchor_closed,
                anchor_closed,
                now_s,
                now_s,
            ),
        )
        wo_id = cur.lastrowid
    print(f"equipment {eq_id}: anchor WO id={wo_id}")

    fe_count = scalar("SELECT COUNT(*) FROM failure_events WHERE equipment_id=?", (eq_id,))
    if fe_count < 8:
        for i in range(8):
            entity_sync_id = f"RAMS-DEMO-FE-{eq_id}-{i}"
            if cur.execute("SELECT 1 FROM failure_events WHERE entity_sync_id=?", (entity_sync_id,)).fetchone():
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
        print(f"equipment {eq_id}: inserted failure events")

    exp_count = scalar(
        "SELECT COUNT(*) FROM runtime_exposure_logs WHERE equipment_id=? AND source_type='rams_demo_seed'",
        (eq_id,),
    )
    if exp_count < 12:
        hours_per_month = 2400.0 / 12.0
        for month in range(12):
            entity_sync_id = f"rams-demo-exp-{eq_id}-{month}"
            if cur.execute(
                "SELECT 1 FROM runtime_exposure_logs WHERE entity_sync_id=?", (entity_sync_id,)
            ).fetchone():
                continue
            recorded_at = (now - timedelta(days=(11 - month) * 28)).strftime("%Y-%m-%dT%H:%M:%SZ")
            cur.execute(
                "INSERT INTO runtime_exposure_logs (entity_sync_id, equipment_id, exposure_type, value, "
                "recorded_at, source_type, row_version) VALUES (?, ?, 'hours', ?, ?, 'rams_demo_seed', 1)",
                (entity_sync_id, eq_id, hours_per_month, recorded_at),
            )
        print(f"equipment {eq_id}: inserted exposure logs")


sc_id = ensure_schedule_class()
for eq_id in EQ_IDS:
    ensure_equipment(eq_id, sc_id)

con.commit()
con.close()
print("Done. Restart app or refresh RAMS dashboard.")
