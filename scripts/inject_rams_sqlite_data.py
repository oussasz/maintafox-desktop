#!/usr/bin/env python3
"""
Direct SQLite RAMS injector for Maintafox.

Purpose:
- Bypass Tauri/Rust ingestion and inject data directly in SQLite.
- Populate 12 months of DI/WO/failure/reliability data for RAMS testing.
- Keep inserts inside a single transaction.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import random
import sqlite3
import sys
import uuid
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable


RANDOM_SEED = 20260426
MARKER = "RAMS-INJECT"
DEFAULT_FTA_GRAPH_JSON = (
    '{"spec_version":1,"top_id":"top","nodes":{"top":{"kind":"or","inputs":["a","b"]},'
    '"a":{"kind":"basic","p":0.01},"b":{"kind":"basic","p":0.02}}}'
)
DEFAULT_RBD_GRAPH_JSON = (
    '{"spec_version":1,"root_id":"root","nodes":{"root":{"kind":"series","children":["x","y"]},'
    '"x":{"kind":"block","r":0.99},"y":{"kind":"block","r":0.95}}}'
)
DEFAULT_EVENT_TREE_GRAPH_JSON = (
    '{"spec_version":1,"initiator":"loss_of_feed","branches":['
    '{"label":"Protection succeeds","probability":0.92,"outcome":"safe_shutdown"},'
    '{"label":"Protection fails","probability":0.08,"outcome":"service_loss"}]}'
)
DEFAULT_MARKOV_GRAPH_JSON = (
    '{"spec_version":1,"kind":"discrete","states":["Up","Degraded","Down"],'
    '"matrix":[[0.94,0.05,0.01],[0.10,0.75,0.15],[0.25,0.50,0.25]]}'
)
DEFAULT_MC_GRAPH_JSON = (
    '{"spec_version":1,"kind":"mc_sample","distribution":{"type":"uniform","low":0.0,"high":1.0}}'
)


@dataclass
class WorkOrderRow:
    wo_id: int
    equipment_id: int
    started_at: dt.datetime
    closed_at: dt.datetime
    failure_interval_hours: float
    repair_hours: float


def iso(ts: dt.datetime) -> str:
    return ts.replace(microsecond=0).strftime("%Y-%m-%dT%H:%M:%SZ")


def month_start_utc(ts: dt.datetime) -> dt.datetime:
    return dt.datetime(ts.year, ts.month, 1, 0, 0, 0, tzinfo=dt.timezone.utc)


def add_months(ts: dt.datetime, months: int) -> dt.datetime:
    total = (ts.year * 12 + ts.month - 1) + months
    year = total // 12
    month = total % 12 + 1
    return dt.datetime(year, month, 1, 0, 0, 0, tzinfo=dt.timezone.utc)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Inject RAMS test data directly into Maintafox SQLite."
    )
    parser.add_argument(
        "--db-path",
        type=str,
        default="",
        help="Optional explicit SQLite path (defaults to discovery).",
    )
    parser.add_argument(
        "--ot-count",
        type=int,
        default=100,
        help="Number of work orders (default: 100).",
    )
    parser.add_argument(
        "--di-count",
        type=int,
        default=120,
        help="Number of intervention requests (default: 120).",
    )
    parser.add_argument(
        "--personnel-count",
        type=int,
        default=5,
        help="Number of personnel records (default: 5).",
    )
    parser.add_argument(
        "--equipment-count",
        type=int,
        default=10,
        help="Number of equipment records (default: 10).",
    )
    parser.add_argument(
        "--months",
        type=int,
        default=12,
        help="How many months of timeline/snapshots to generate.",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Validate path and schema but do not insert.",
    )
    return parser.parse_args()


def discover_db_path(explicit: str) -> Path:
    if explicit:
        p = Path(explicit).expanduser().resolve()
        if not p.exists():
            raise FileNotFoundError(f"Database path does not exist: {p}")
        return p

    candidates = [
        Path("src-tauri/maintafox.db"),
        Path("src-tauri/dev.db"),
        Path("maintafox.db"),
        Path("dev-data/maintafox.db"),
    ]
    for candidate in candidates:
        if candidate.exists():
            return candidate.resolve()

    raise FileNotFoundError(
        "No database found. Tried: "
        + ", ".join(str(c) for c in candidates)
        + ". Use --db-path."
    )


def fetch_one(cur: sqlite3.Cursor, sql: str, params: Iterable[object] = ()) -> sqlite3.Row | None:
    cur.execute(sql, tuple(params))
    return cur.fetchone()


def ensure_admin_user(cur: sqlite3.Cursor, now: dt.datetime) -> int:
    row = fetch_one(cur, "SELECT id FROM user_accounts WHERE username = 'admin' LIMIT 1")
    if row:
        return int(row["id"])

    cur.execute(
        """
        INSERT INTO user_accounts
            (sync_id, username, display_name, identity_mode, password_hash,
             is_active, is_admin, force_password_change, failed_login_attempts,
             created_at, updated_at, row_version)
        VALUES (?, 'admin', 'Admin (RAMS)', 'local', '', 1, 1, 0, 0, ?, ?, 1)
        """,
        (str(uuid.uuid4()), iso(now), iso(now)),
    )
    return int(cur.lastrowid)


def ensure_structure_model(cur: sqlite3.Cursor, now: dt.datetime) -> int:
    row = fetch_one(cur, "SELECT id FROM org_structure_models WHERE status = 'active' ORDER BY id LIMIT 1")
    if row:
        return int(row["id"])
    cur.execute(
        """
        INSERT INTO org_structure_models
            (sync_id, version_number, status, description, activated_at, created_at, updated_at)
        VALUES (?, 1, 'active', ?, ?, ?, ?)
        """,
        (str(uuid.uuid4()), f"{MARKER} structure model", iso(now), iso(now), iso(now)),
    )
    return int(cur.lastrowid)


def ensure_root_node_type(cur: sqlite3.Cursor, structure_model_id: int, now: dt.datetime) -> int:
    row = fetch_one(cur, "SELECT id FROM org_node_types WHERE code = ? LIMIT 1", ("RAMS_SITE",))
    if row:
        return int(row["id"])
    cur.execute(
        """
        INSERT INTO org_node_types
            (sync_id, structure_model_id, code, label, can_host_assets, can_own_work, is_root_type, created_at, updated_at)
        VALUES (?, ?, 'RAMS_SITE', 'RAMS Site', 1, 1, 1, ?, ?)
        """,
        (str(uuid.uuid4()), structure_model_id, iso(now), iso(now)),
    )
    return int(cur.lastrowid)


def ensure_org_node(cur: sqlite3.Cursor, node_type_id: int, now: dt.datetime) -> int:
    row = fetch_one(cur, "SELECT id FROM org_nodes WHERE code = ? LIMIT 1", ("RAMS-ROOT",))
    if row:
        return int(row["id"])
    cur.execute(
        """
        INSERT INTO org_nodes
            (sync_id, code, name, node_type_id, parent_id, ancestor_path, depth, status, created_at, updated_at, row_version)
        VALUES (?, 'RAMS-ROOT', ?, ?, NULL, '/', 0, 'active', ?, ?, 1)
        """,
        (str(uuid.uuid4()), "RAMS Root Entity", node_type_id, iso(now), iso(now)),
    )
    return int(cur.lastrowid)


def ensure_equipment_class(cur: sqlite3.Cursor, now: dt.datetime) -> int:
    row = fetch_one(cur, "SELECT id FROM equipment_classes WHERE code = ? LIMIT 1", ("RAMS_CLASS",))
    if row:
        return int(row["id"])
    cur.execute(
        """
        INSERT INTO equipment_classes
            (sync_id, code, name, parent_id, level, is_active, created_at, updated_at)
        VALUES (?, 'RAMS_CLASS', 'RAMS Equipment Class', NULL, 'class', 1, ?, ?)
        """,
        (str(uuid.uuid4()), iso(now), iso(now)),
    )
    return int(cur.lastrowid)


def ensure_wo_lookup_ids(cur: sqlite3.Cursor) -> tuple[int, int, int]:
    type_row = fetch_one(
        cur,
        "SELECT id FROM work_order_types WHERE code = 'corrective' LIMIT 1",
    )
    if not type_row:
        raise RuntimeError("Missing work_order_types.corrective")

    status_row = fetch_one(
        cur,
        "SELECT id FROM work_order_statuses WHERE code = 'closed' LIMIT 1",
    )
    if not status_row:
        raise RuntimeError("Missing work_order_statuses.closed")

    urgency_row = fetch_one(
        cur,
        "SELECT id FROM urgency_levels WHERE level = 4 LIMIT 1",
    ) or fetch_one(cur, "SELECT id FROM urgency_levels ORDER BY id LIMIT 1")
    if not urgency_row:
        raise RuntimeError("Missing urgency_levels rows")

    return int(type_row["id"]), int(status_row["id"]), int(urgency_row["id"])


def ensure_personnel(cur: sqlite3.Cursor, org_node_id: int, count: int, now: dt.datetime) -> list[int]:
    cur.execute(
        "SELECT id FROM personnel WHERE employee_code LIKE ? ORDER BY id",
        (f"{MARKER}-%",),
    )
    existing = [int(r["id"]) for r in cur.fetchall()]
    if len(existing) >= count:
        return existing[:count]

    needed = count - len(existing)
    for i in range(needed):
        idx = len(existing) + i + 1
        code = f"{MARKER}-{idx:03d}"
        cur.execute(
            """
            INSERT INTO personnel
                (employee_code, full_name, employment_type, primary_entity_id, availability_status, email, row_version, created_at, updated_at)
            VALUES (?, ?, 'employee', ?, 'available', ?, 1, ?, ?)
            """,
            (code, f"RAMS Tech {idx}", org_node_id, f"rams.tech.{idx}@local.invalid", iso(now), iso(now)),
        )
        existing.append(int(cur.lastrowid))
    return existing


def ensure_equipment(
    cur: sqlite3.Cursor, org_node_id: int, class_id: int, count: int, now: dt.datetime
) -> list[int]:
    cur.execute(
        "SELECT id FROM equipment WHERE asset_id_code LIKE ? ORDER BY id",
        (f"{MARKER}-EQ-%",),
    )
    existing = [int(r["id"]) for r in cur.fetchall()]
    if len(existing) >= count:
        return existing[:count]

    needed = count - len(existing)
    for i in range(needed):
        idx = len(existing) + i + 1
        code = f"{MARKER}-EQ-{idx:03d}"
        cur.execute(
            """
            INSERT INTO equipment
                (sync_id, asset_id_code, name, class_id, lifecycle_status, installed_at_node_id, manufacturer, created_at, updated_at, row_version)
            VALUES (?, ?, ?, ?, 'active_in_service', ?, 'RAMS-LAB', ?, ?, 1)
            """,
            (str(uuid.uuid4()), code, f"RAMS Equipment {idx}", class_id, org_node_id, iso(now), iso(now)),
        )
        existing.append(int(cur.lastrowid))
    return existing


def clear_previous_injection(cur: sqlite3.Cursor) -> None:
    cur.execute("SELECT id FROM intervention_requests WHERE code LIKE ?", (f"{MARKER}-DI-%",))
    di_ids = [int(r["id"]) for r in cur.fetchall()]
    cur.execute("SELECT id FROM work_orders WHERE code LIKE ?", (f"{MARKER}-OT-%",))
    wo_ids = [int(r["id"]) for r in cur.fetchall()]

    if wo_ids:
        marks = ",".join("?" for _ in wo_ids)
        cur.execute(f"DELETE FROM failure_events WHERE source_type = 'work_order' AND source_id IN ({marks})", wo_ids)
    if di_ids:
        marks = ",".join("?" for _ in di_ids)
        cur.execute(f"DELETE FROM di_state_transition_log WHERE di_id IN ({marks})", di_ids)
    if wo_ids:
        marks = ",".join("?" for _ in wo_ids)
        cur.execute(f"DELETE FROM wo_state_transition_log WHERE wo_id IN ({marks})", wo_ids)

    cur.execute("DELETE FROM reliability_kpi_snapshots WHERE entity_sync_id LIKE ?", (f"{MARKER}-%",))
    cur.execute("DELETE FROM runtime_exposure_logs WHERE entity_sync_id LIKE ?", (f"{MARKER}-%",))
    cur.execute("DELETE FROM weibull_fit_results WHERE entity_sync_id LIKE ?", (f"{MARKER}-%",))
    cur.execute("DELETE FROM ram_expert_sign_offs WHERE entity_sync_id LIKE ?", (f"{MARKER}-%",))
    cur.execute("DELETE FROM mc_models WHERE entity_sync_id LIKE ?", (f"{MARKER}-%",))
    cur.execute("DELETE FROM markov_models WHERE entity_sync_id LIKE ?", (f"{MARKER}-%",))
    cur.execute("DELETE FROM event_tree_models WHERE entity_sync_id LIKE ?", (f"{MARKER}-%",))
    cur.execute("DELETE FROM fta_models WHERE entity_sync_id LIKE ?", (f"{MARKER}-%",))
    cur.execute("DELETE FROM rbd_models WHERE entity_sync_id LIKE ?", (f"{MARKER}-%",))
    cur.execute("DELETE FROM ram_ishikawa_diagrams WHERE entity_sync_id LIKE ?", (f"{MARKER}-%",))
    cur.execute("DELETE FROM fmeca_items WHERE entity_sync_id LIKE ?", (f"{MARKER}-%",))
    cur.execute("DELETE FROM fmeca_analyses WHERE entity_sync_id LIKE ?", (f"{MARKER}-%",))
    cur.execute("DELETE FROM work_orders WHERE code LIKE ?", (f"{MARKER}-OT-%",))
    cur.execute("DELETE FROM intervention_requests WHERE code LIKE ?", (f"{MARKER}-DI-%",))


def generate_and_insert_di(
    cur: sqlite3.Cursor,
    di_count: int,
    months: int,
    equipment_ids: list[int],
    org_node_id: int,
    submitter_id: int,
    now: dt.datetime,
    rng: random.Random,
) -> list[tuple[int, dt.datetime]]:
    month0 = month_start_utc(now) - dt.timedelta(days=30 * (months - 1))
    di_rows: list[tuple[int, dt.datetime]] = []

    for i in range(di_count):
        code = f"{MARKER}-DI-{i+1:04d}"
        equipment_id = equipment_ids[i % len(equipment_ids)]

        month_offset = i % months
        base = add_months(month0, month_offset)
        created_at = base + dt.timedelta(days=rng.randint(0, 25), hours=rng.randint(0, 20))
        submitted_at = created_at + dt.timedelta(minutes=rng.randint(5, 180))
        urgency = rng.choice(["medium", "high", "critical"])

        cur.execute(
            """
            INSERT INTO intervention_requests
                (code, asset_id, org_node_id, status, title, description, origin_type,
                 impact_level, production_impact, safety_flag, environmental_flag, quality_flag,
                 reported_urgency, submitted_at, submitter_id, row_version, created_at, updated_at)
            VALUES (?, ?, ?, 'approved_for_planning', ?, ?, 'operator',
                    'medium', 1, 0, 0, 0, ?, ?, ?, 1, ?, ?)
            """,
            (
                code,
                equipment_id,
                org_node_id,
                f"RAMS injected DI {i+1}",
                "Injected directly for RAMS testing",
                urgency,
                iso(submitted_at),
                submitter_id,
                iso(created_at),
                iso(created_at),
            ),
        )
        di_id = int(cur.lastrowid)
        di_rows.append((di_id, created_at))

        cur.execute(
            """
            INSERT INTO di_state_transition_log (di_id, from_status, to_status, action, actor_id, notes, acted_at)
            VALUES (?, 'submitted', 'approved_for_planning', 'inject_seed', ?, 'Direct RAMS injection', ?)
            """,
            (di_id, submitter_id, iso(submitted_at)),
        )

    return di_rows


def generate_and_insert_work_orders(
    cur: sqlite3.Cursor,
    ot_count: int,
    months: int,
    equipment_ids: list[int],
    org_node_id: int,
    requester_id: int,
    planner_id: int,
    wo_type_id: int,
    wo_status_id: int,
    urgency_id: int,
    di_rows: list[tuple[int, dt.datetime]],
    now: dt.datetime,
    rng: random.Random,
) -> list[WorkOrderRow]:
    month0 = month_start_utc(now) - dt.timedelta(days=30 * (months - 1))
    last_failure_by_eq: dict[int, dt.datetime] = {}
    results: list[WorkOrderRow] = []

    for i in range(ot_count):
        equipment_id = equipment_ids[i % len(equipment_ids)]
        source_di_id, di_created_at = di_rows[i]
        failure_gap_h = rng.uniform(150.0, 400.0)
        repair_h = rng.uniform(1.0, 6.0)
        waiting_h = rng.uniform(0.1, 8.0)
        downtime_h = repair_h + waiting_h

        previous_failure = last_failure_by_eq.get(equipment_id)
        if previous_failure is None:
            month_offset = i % months
            start_base = add_months(month0, month_offset) + dt.timedelta(days=rng.randint(0, 20))
            started_at = max(start_base, di_created_at + dt.timedelta(hours=1))
        else:
            started_at = previous_failure + dt.timedelta(hours=failure_gap_h)
            started_at = max(started_at, di_created_at + dt.timedelta(hours=1))

        closed_at = started_at + dt.timedelta(hours=downtime_h)
        planned_start = started_at - dt.timedelta(hours=rng.uniform(2.0, 24.0))
        planned_end = closed_at

        code = f"{MARKER}-OT-{i+1:04d}"
        entity_sync_id = str(uuid.uuid4())
        title = f"RAMS corrective order #{i+1}"

        cur.execute(
            """
            INSERT INTO work_orders
                (code, type_id, status_id, equipment_id, source_di_id, entity_id,
                 requester_id, planner_id, urgency_id, title, description,
                 planned_start, planned_end, actual_start, actual_end,
                 closed_at, expected_duration_hours, actual_duration_hours,
                 active_labor_hours, total_waiting_hours, downtime_hours,
                 labor_cost, parts_cost, service_cost, total_cost,
                 row_version, created_at, updated_at,
                 entity_sync_id, closeout_validation_passed, no_downtime_attestation, requires_permit)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?,
                    ?, ?, ?, ?, ?, ?, ?, ?, ?, ?,
                    ?, ?, ?, ?, 1, ?, ?,
                    ?, 1, 0, 0)
            """,
            (
                code,
                wo_type_id,
                wo_status_id,
                equipment_id,
                source_di_id,
                org_node_id,
                requester_id,
                planner_id,
                urgency_id,
                title,
                "Injected directly for RAMS MTBF/MTTR testing",
                iso(planned_start),
                iso(planned_end),
                iso(started_at),
                iso(closed_at),
                iso(closed_at),
                downtime_h + rng.uniform(0.5, 2.0),
                downtime_h,
                repair_h,
                waiting_h,
                downtime_h,
                round(repair_h * 42.0, 2),
                round(rng.uniform(20.0, 180.0), 2),
                round(rng.uniform(0.0, 70.0), 2),
                round(repair_h * 42.0 + rng.uniform(20.0, 250.0), 2),
                iso(started_at - dt.timedelta(hours=2)),
                iso(closed_at),
                entity_sync_id,
            ),
        )
        wo_id = int(cur.lastrowid)
        results.append(
            WorkOrderRow(
                wo_id=wo_id,
                equipment_id=equipment_id,
                started_at=started_at,
                closed_at=closed_at,
                failure_interval_hours=failure_gap_h,
                repair_hours=repair_h,
            )
        )

        cur.execute(
            """
            INSERT INTO wo_state_transition_log
                (wo_id, from_status, to_status, action, actor_id, notes, acted_at)
            VALUES (?, 'in_progress', 'closed', 'inject_seed', ?, 'Direct RAMS injection', ?)
            """,
            (wo_id, planner_id, iso(closed_at)),
        )

        last_failure_by_eq[equipment_id] = started_at

    return results


def insert_failure_events(
    cur: sqlite3.Cursor, wo_rows: list[WorkOrderRow], actor_user_id: int, rng: random.Random
) -> None:
    for wo in wo_rows:
        detected_at = wo.started_at - dt.timedelta(hours=rng.uniform(0.2, 1.5))
        cur.execute(
            """
            INSERT INTO failure_events
                (entity_sync_id, source_type, source_id, equipment_id,
                 detected_at, failed_at, restored_at,
                 downtime_duration_hours, active_repair_hours, waiting_hours,
                 is_planned, cause_not_determined, production_impact_level, safety_impact_level,
                 recorded_by_id, verification_status, eligible_flags_json, row_version,
                 created_at, updated_at)
            VALUES (?, 'work_order', ?, ?,
                    ?, ?, ?,
                    ?, ?, ?,
                    0, 0, 2, 1,
                    ?, 'verified', ?, 1,
                    ?, ?)
            """,
            (
                str(uuid.uuid4()),
                wo.wo_id,
                wo.equipment_id,
                iso(detected_at),
                iso(wo.started_at),
                iso(wo.closed_at),
                wo.repair_hours + rng.uniform(0.1, 3.0),
                wo.repair_hours,
                rng.uniform(0.1, 3.5),
                actor_user_id,
                json.dumps({"ot_linked": True, "rams_injected": True}),
                iso(wo.closed_at),
                iso(wo.closed_at),
            ),
        )


def insert_reliability_snapshots(
    cur: sqlite3.Cursor,
    wo_rows: list[WorkOrderRow],
    equipment_ids: list[int],
    months: int,
    now: dt.datetime,
) -> None:
    month0 = month_start_utc(now) - dt.timedelta(days=30 * (months - 1))

    by_eq_month: dict[tuple[int, str], list[WorkOrderRow]] = {}
    for row in wo_rows:
        key = (row.equipment_id, row.closed_at.strftime("%Y-%m"))
        by_eq_month.setdefault(key, []).append(row)

    for equipment_id in equipment_ids:
        for idx in range(months):
            start = add_months(month0, idx)
            end = add_months(start, 1) - dt.timedelta(seconds=1)
            ym = start.strftime("%Y-%m")
            rows = by_eq_month.get((equipment_id, ym), [])
            event_count = len(rows)

            if event_count > 0:
                mtbf = sum(r.failure_interval_hours for r in rows) / event_count
                mttr = sum(r.repair_hours for r in rows) / event_count
                failure_rate = 1.0 / mtbf if mtbf > 0 else None
            else:
                mtbf = None
                mttr = None
                failure_rate = None

            availability = None
            if mtbf is not None and mttr is not None and (mtbf + mttr) > 0:
                availability = mtbf / (mtbf + mttr)

            quality = 0.97 if event_count > 0 else 0.90
            repeat_rate = 0.05 if event_count > 1 else 0.0
            inspection_signal = json.dumps(
                {
                    "source": MARKER,
                    "month": ym,
                    "event_count": event_count,
                }
            )
            plot_payload = json.dumps(
                {
                    "series": [
                        {"name": "MTBF", "value": mtbf},
                        {"name": "MTTR", "value": mttr},
                        {"name": "Availability", "value": availability},
                    ],
                    "month": ym,
                }
            )
            analysis_spec = json.dumps(
                {"generator": MARKER, "equipment_id": equipment_id, "period": ym}
            )

            cur.execute(
                """
                INSERT INTO reliability_kpi_snapshots
                    (entity_sync_id, equipment_id, period_start, period_end,
                     mtbf, mttr, availability, failure_rate, repeat_failure_rate,
                     event_count, data_quality_score, inspection_signal_json, row_version,
                     analysis_dataset_hash_sha256, analysis_input_spec_json, plot_payload_json)
                VALUES (?, ?, ?, ?,
                        ?, ?, ?, ?, ?,
                        ?, ?, ?, 1,
                        ?, ?, ?)
                """,
                (
                    f"{MARKER}-{equipment_id}-{ym}",
                    equipment_id,
                    iso(start),
                    iso(end),
                    mtbf,
                    mttr,
                    availability,
                    failure_rate,
                    repeat_rate,
                    event_count,
                    quality,
                    inspection_signal,
                    "",
                    analysis_spec,
                    plot_payload,
                ),
            )


def insert_runtime_exposure_logs(
    cur: sqlite3.Cursor,
    equipment_ids: list[int],
    months: int,
    now: dt.datetime,
    rng: random.Random,
) -> int:
    month0 = month_start_utc(now) - dt.timedelta(days=30 * (months - 1))
    inserted = 0
    for equipment_id in equipment_ids:
        for idx in range(months):
            period_start = add_months(month0, idx)
            hours_value = rng.uniform(180.0, 520.0)
            cur.execute(
                """
                INSERT INTO runtime_exposure_logs
                    (entity_sync_id, equipment_id, exposure_type, value, recorded_at, source_type, row_version)
                VALUES (?, ?, 'operating_hours', ?, ?, 'rams_injector', 1)
                """,
                (
                    f"{MARKER}-EXPO-{equipment_id}-{period_start.strftime('%Y%m')}",
                    equipment_id,
                    hours_value,
                    iso(period_start + dt.timedelta(days=27, hours=23)),
                ),
            )
            inserted += 1
    return inserted


def build_ishikawa_flow_json(equipment_id: int, rng: random.Random) -> str:
    nodes = [
        {"id": "effect", "type": "default", "position": {"x": 520, "y": 220}, "data": {"label": "Effect"}},
        {"id": "cat_machine", "type": "default", "position": {"x": 40, "y": 40}, "data": {"label": "Machine"}},
        {"id": "cat_method", "type": "default", "position": {"x": 180, "y": 20}, "data": {"label": "Method"}},
        {"id": "cat_material", "type": "default", "position": {"x": 320, "y": 40}, "data": {"label": "Material"}},
        {"id": "cat_manpower", "type": "default", "position": {"x": 40, "y": 360}, "data": {"label": "Manpower"}},
        {"id": "cat_measurement", "type": "default", "position": {"x": 220, "y": 400}, "data": {"label": "Measurement"}},
        {"id": "cat_nature", "type": "default", "position": {"x": 380, "y": 360}, "data": {"label": "Nature"}},
    ]
    edges = [
        {"id": "e_cat_machine", "source": "cat_machine", "target": "effect", "type": "smoothstep"},
        {"id": "e_cat_method", "source": "cat_method", "target": "effect", "type": "smoothstep"},
        {"id": "e_cat_material", "source": "cat_material", "target": "effect", "type": "smoothstep"},
        {"id": "e_cat_manpower", "source": "cat_manpower", "target": "effect", "type": "smoothstep"},
        {"id": "e_cat_measurement", "source": "cat_measurement", "target": "effect", "type": "smoothstep"},
        {"id": "e_cat_nature", "source": "cat_nature", "target": "effect", "type": "smoothstep"},
    ]

    categories = ["cat_machine", "cat_method", "cat_material", "cat_manpower", "cat_measurement", "cat_nature"]
    for idx in range(1, 5):
        cat = categories[(equipment_id + idx) % len(categories)]
        cause_id = f"cause_{equipment_id}_{idx}"
        nodes.append(
            {
                "id": cause_id,
                "type": "default",
                "position": {"x": 120 + idx * 40 + rng.randint(-20, 20), "y": 80 + idx * 55 + rng.randint(-15, 15)},
                "data": {"label": f"Cause {idx} EQ-{equipment_id}"},
            }
        )
        edges.append({"id": f"e_{cause_id}", "source": cat, "target": cause_id, "type": "smoothstep"})

    payload = {
        "spec_version": 1,
        "nodes": nodes,
        "edges": edges,
        "viewport": {"x": 0, "y": 0, "zoom": 1},
    }
    return json.dumps(payload)


def insert_ishikawa_seed(cur: sqlite3.Cursor, equipment_ids: list[int], now: dt.datetime, rng: random.Random) -> dict[int, int]:
    diagram_ids: dict[int, int] = {}
    for equipment_id in equipment_ids:
        cur.execute(
            """
            INSERT INTO ram_ishikawa_diagrams
                (entity_sync_id, equipment_id, title, flow_json, row_version, created_at, updated_at)
            VALUES (?, ?, ?, ?, 1, ?, ?)
            """,
            (
                f"{MARKER}-ISHI-{equipment_id}",
                equipment_id,
                f"{MARKER} Ishikawa EQ-{equipment_id}",
                build_ishikawa_flow_json(equipment_id, rng),
                iso(now),
                iso(now),
            ),
        )
        diagram_ids[equipment_id] = int(cur.lastrowid)
    return diagram_ids


def insert_fmeca_seed(
    cur: sqlite3.Cursor,
    equipment_ids: list[int],
    wo_rows: list[WorkOrderRow],
    ishikawa_ids: dict[int, int],
    now: dt.datetime,
    rng: random.Random,
) -> tuple[int, int]:
    """Seed FMECA analyses/items so the 10x10 matrix has non-zero cells."""
    wo_by_equipment: dict[int, list[WorkOrderRow]] = {}
    for wo in wo_rows:
        wo_by_equipment.setdefault(wo.equipment_id, []).append(wo)

    analyses_count = 0
    items_count = 0
    for equipment_id in equipment_ids:
        analysis_sync_id = f"{MARKER}-FMECA-AN-{equipment_id}-{uuid.uuid4()}"
        cur.execute(
            """
            INSERT INTO fmeca_analyses
                (entity_sync_id, equipment_id, title, boundary_definition, status, row_version, created_at, created_by_id, updated_at)
            VALUES (?, ?, ?, 'Injected scope', 'active', 1, ?, NULL, ?)
            """,
            (
                analysis_sync_id,
                equipment_id,
                f"{MARKER} FMECA EQ-{equipment_id}",
                iso(now),
                iso(now),
            ),
        )
        analysis_id = int(cur.lastrowid)
        analyses_count += 1

        equipment_wos = wo_by_equipment.get(equipment_id, [])
        base_count = max(6, min(14, len(equipment_wos)))
        for idx in range(base_count):
            wo_link = equipment_wos[idx % len(equipment_wos)].wo_id if equipment_wos else None
            source_diagram_id = ishikawa_ids.get(equipment_id)
            source_flow_node = f"cause_{equipment_id}_{(idx % 4) + 1}" if source_diagram_id else None
            severity = rng.randint(1, 10)
            occurrence = rng.randint(1, 10)
            detectability = rng.randint(1, 10)
            rpn = severity * occurrence * detectability
            item_sync_id = f"{MARKER}-FMECA-IT-{equipment_id}-{idx+1:03d}-{uuid.uuid4()}"
            cur.execute(
                """
                INSERT INTO fmeca_items
                    (entity_sync_id, analysis_id, component_id, functional_failure, failure_mode_id, failure_effect,
                     severity, occurrence, detectability, rpn, recommended_action, current_control,
                     linked_pm_plan_id, linked_work_order_id, revised_rpn,
                     source_ram_ishikawa_diagram_id, source_ishikawa_flow_node_id,
                     row_version, updated_at)
                VALUES (?, ?, NULL, ?, NULL, ?,
                        ?, ?, ?, ?, ?, ?,
                        NULL, ?, NULL, ?, ?, 1, ?)
                """,
                (
                    item_sync_id,
                    analysis_id,
                    f"Injected failure mode #{idx+1}",
                    "Loss of intended function under RAMS scenario",
                    severity,
                    occurrence,
                    detectability,
                    rpn,
                    "Condition-based intervention and spare readiness",
                    "Visual inspection + alarm threshold monitoring",
                    wo_link,
                    source_diagram_id,
                    source_flow_node,
                    iso(now),
                ),
            )
            items_count += 1

    return analyses_count, items_count


def insert_models_seed(
    cur: sqlite3.Cursor,
    equipment_ids: list[int],
    ishikawa_ids: dict[int, int],
    now: dt.datetime,
    rng: random.Random,
) -> dict[str, int]:
    counts = {
        "fta": 0,
        "rbd": 0,
        "event_tree": 0,
        "mc": 0,
        "markov": 0,
        "expert_signoff": 0,
        "weibull": 0,
    }
    for equipment_id in equipment_ids:
        ts = iso(now)
        # FTA
        cur.execute(
            """
            INSERT INTO fta_models
                (entity_sync_id, equipment_id, title, graph_json, result_json, status, row_version, created_at, created_by_id, updated_at)
            VALUES (?, ?, ?, ?, '{"top_event_probability":0.0298}', 'active', 1, ?, NULL, ?)
            """,
            (f"{MARKER}-FTA-{equipment_id}", equipment_id, f"{MARKER} FTA EQ-{equipment_id}", DEFAULT_FTA_GRAPH_JSON, ts, ts),
        )
        counts["fta"] += 1

        # RBD
        cur.execute(
            """
            INSERT INTO rbd_models
                (entity_sync_id, equipment_id, title, graph_json, result_json, status, row_version, created_at, created_by_id, updated_at)
            VALUES (?, ?, ?, ?, '{"availability":0.9405}', 'active', 1, ?, NULL, ?)
            """,
            (f"{MARKER}-RBD-{equipment_id}", equipment_id, f"{MARKER} RBD EQ-{equipment_id}", DEFAULT_RBD_GRAPH_JSON, ts, ts),
        )
        counts["rbd"] += 1

        # Event tree
        cur.execute(
            """
            INSERT INTO event_tree_models
                (entity_sync_id, equipment_id, title, graph_json, result_json, status, row_version, created_at, created_by_id, updated_at)
            VALUES (?, ?, ?, ?, '{"safe_shutdown":0.92,"service_loss":0.08}', 'active', 1, ?, NULL, ?)
            """,
            (
                f"{MARKER}-ETA-{equipment_id}",
                equipment_id,
                f"{MARKER} ETA EQ-{equipment_id}",
                DEFAULT_EVENT_TREE_GRAPH_JSON,
                ts,
                ts,
            ),
        )
        counts["event_tree"] += 1

        # Monte Carlo
        cur.execute(
            """
            INSERT INTO mc_models
                (entity_sync_id, equipment_id, title, graph_json, trials, seed, result_json, status, row_version, created_at, created_by_id, updated_at)
            VALUES (?, ?, ?, ?, 5000, ?, '{"sample_mean":0.50,"sample_std":0.29,"p05":0.05,"p95":0.95}', 'active', 1, ?, NULL, ?)
            """,
            (
                f"{MARKER}-MC-{equipment_id}",
                equipment_id,
                f"{MARKER} MC EQ-{equipment_id}",
                DEFAULT_MC_GRAPH_JSON,
                rng.randint(1, 10_000_000),
                ts,
                ts,
            ),
        )
        counts["mc"] += 1

        # Markov (must be valid, dashboard evaluates it on load)
        cur.execute(
            """
            INSERT INTO markov_models
                (entity_sync_id, equipment_id, title, graph_json, result_json, status, row_version, created_at, created_by_id, updated_at)
            VALUES (?, ?, ?, ?, '{"steady_state":[0.79,0.16,0.05],"iterations":43}', 'active', 1, ?, NULL, ?)
            """,
            (
                f"{MARKER}-MARKOV-{equipment_id}",
                equipment_id,
                f"{MARKER} Markov EQ-{equipment_id}",
                DEFAULT_MARKOV_GRAPH_JSON,
                ts,
                ts,
            ),
        )
        counts["markov"] += 1

        # Expert sign-off
        cur.execute(
            """
            INSERT INTO ram_expert_sign_offs
                (entity_sync_id, equipment_id, method_category, target_ref, title, reviewer_name, reviewer_role,
                 status, signed_at, notes, row_version, created_at, created_by_id, updated_at)
            VALUES (?, ?, 'ishikawa', ?, ?, 'RAMS Reviewer', 'Reliability Engineer',
                    'signed', ?, ?, 1, ?, NULL, ?)
            """,
            (
                f"{MARKER}-SIGNOFF-{equipment_id}",
                equipment_id,
                f"diagram:{ishikawa_ids.get(equipment_id, 0)}",
                f"{MARKER} Governance Sign-off EQ-{equipment_id}",
                ts,
                "Reviewed seeded Ishikawa and linked mitigation records.",
                ts,
                ts,
            ),
        )
        counts["expert_signoff"] += 1

        # Baseline Weibull row so dashboard has immediate curve params.
        inter = [round(rng.uniform(150.0, 400.0), 3) for _ in range(18)]
        cur.execute(
            """
            INSERT INTO weibull_fit_results
                (entity_sync_id, equipment_id, period_start, period_end, n_points, inter_arrival_hours_json,
                 beta, eta, beta_ci_low, beta_ci_high, eta_ci_low, eta_ci_high,
                 adequate_sample, message, row_version, created_at, created_by_id)
            VALUES (?, ?, NULL, NULL, ?, ?, 1.45, 265.0, 1.20, 1.72, 240.0, 298.0,
                    1, 'Seeded baseline fit for demo', 1, ?, NULL)
            """,
            (
                f"{MARKER}-WEIBULL-{equipment_id}",
                equipment_id,
                len(inter),
                json.dumps(inter),
                ts,
            ),
        )
        counts["weibull"] += 1

    return counts


def main() -> int:
    args = parse_args()
    randomizer = random.Random(RANDOM_SEED)
    now = dt.datetime.now(dt.timezone.utc)

    try:
        db_path = discover_db_path(args.db_path)
    except Exception as exc:
        print(f"[ERROR] {exc}", file=sys.stderr)
        return 2

    print(f"[INFO] Using SQLite file: {db_path}")
    conn = sqlite3.connect(str(db_path))
    conn.row_factory = sqlite3.Row

    try:
        conn.execute("PRAGMA foreign_keys = ON")
        cur = conn.cursor()

        if args.dry_run:
            print("[INFO] Dry-run mode enabled; no inserts performed.")
            return 0

        conn.execute("BEGIN")

        admin_id = ensure_admin_user(cur, now)
        model_id = ensure_structure_model(cur, now)
        node_type_id = ensure_root_node_type(cur, model_id, now)
        org_node_id = ensure_org_node(cur, node_type_id, now)
        eq_class_id = ensure_equipment_class(cur, now)
        wo_type_id, wo_status_id, urgency_id = ensure_wo_lookup_ids(cur)

        personnel_ids = ensure_personnel(cur, org_node_id, args.personnel_count, now)
        equipment_ids = ensure_equipment(cur, org_node_id, eq_class_id, args.equipment_count, now)

        clear_previous_injection(cur)

        di_rows = generate_and_insert_di(
            cur=cur,
            di_count=args.di_count,
            months=args.months,
            equipment_ids=equipment_ids,
            org_node_id=org_node_id,
            submitter_id=admin_id,
            now=now,
            rng=randomizer,
        )
        wo_rows = generate_and_insert_work_orders(
            cur=cur,
            ot_count=args.ot_count,
            months=args.months,
            equipment_ids=equipment_ids,
            org_node_id=org_node_id,
            requester_id=admin_id,
            planner_id=admin_id,
            wo_type_id=wo_type_id,
            wo_status_id=wo_status_id,
            urgency_id=urgency_id,
            di_rows=di_rows,
            now=now,
            rng=randomizer,
        )
        insert_failure_events(cur, wo_rows, actor_user_id=admin_id, rng=randomizer)
        insert_reliability_snapshots(cur, wo_rows, equipment_ids, args.months, now)
        exposure_count = insert_runtime_exposure_logs(cur, equipment_ids, args.months, now, randomizer)
        ishikawa_ids = insert_ishikawa_seed(cur, equipment_ids, now, randomizer)
        fmeca_analyses_count, fmeca_items_count = insert_fmeca_seed(
            cur=cur,
            equipment_ids=equipment_ids,
            wo_rows=wo_rows,
            ishikawa_ids=ishikawa_ids,
            now=now,
            rng=randomizer,
        )
        model_counts = insert_models_seed(cur, equipment_ids, ishikawa_ids, now, randomizer)

        conn.commit()

        print("[OK] RAMS injection complete.")
        print(f"      Org nodes ensured: 1 (root={org_node_id})")
        print(f"      Personnel ensured: {len(personnel_ids)}")
        print(f"      Equipment ensured: {len(equipment_ids)}")
        print(f"      DIs inserted: {len(di_rows)}")
        print(f"      OTs inserted: {len(wo_rows)}")
        print(f"      Failure events inserted: {len(wo_rows)}")
        print(f"      KPI snapshots inserted: {len(equipment_ids) * args.months}")
        print(f"      Runtime exposure logs inserted: {exposure_count}")
        print(f"      Ishikawa diagrams inserted: {len(ishikawa_ids)}")
        print(f"      FMECA analyses inserted: {fmeca_analyses_count}")
        print(f"      FMECA items inserted: {fmeca_items_count}")
        print(f"      FTA models inserted: {model_counts['fta']}")
        print(f"      RBD models inserted: {model_counts['rbd']}")
        print(f"      Event-tree models inserted: {model_counts['event_tree']}")
        print(f"      MC models inserted: {model_counts['mc']}")
        print(f"      Markov models inserted: {model_counts['markov']}")
        print(f"      Expert sign-offs inserted: {model_counts['expert_signoff']}")
        print(f"      Weibull fit rows inserted: {model_counts['weibull']}")
        print("      MTBF per event constrained to 150..400h")
        print("      MTTR (active_repair_hours) constrained to 1..6h")
        print("      Ordering enforced: DI.created_at < OT.actual_start < OT.closed_at")
        return 0
    except Exception as exc:
        conn.rollback()
        print(f"[ERROR] Injection failed and rolled back: {exc}", file=sys.stderr)
        return 1
    finally:
        conn.close()


if __name__ == "__main__":
    raise SystemExit(main())
