import sqlite3
import sys

db = sys.argv[1] if len(sys.argv) > 1 else r"C:\Users\LENOVO\AppData\Roaming\systems.maintafox.desktop\maintafox.db"
eq_id = int(sys.argv[2]) if len(sys.argv) > 2 else 71

con = sqlite3.connect(db)
cur = con.cursor()

queries = [
    ("equipment", f"SELECT id, rams_schedule_reference_value_id, rams_utilization_factor, installed_at_node_id FROM equipment WHERE id={eq_id}"),
    ("closed_wo", f"SELECT COUNT(*) FROM work_orders WHERE equipment_id={eq_id} AND closed_at IS NOT NULL AND closed_at <= datetime('now') AND code NOT LIKE 'RAMS-INJECT%'"),
    ("failure_events", f"SELECT COUNT(*) FROM failure_events WHERE equipment_id={eq_id}"),
    ("exposure_hours", f"SELECT COALESCE(SUM(value),0) FROM runtime_exposure_logs WHERE equipment_id={eq_id} AND exposure_type='hours'"),
    ("demo_exposure", f"SELECT COALESCE(SUM(value),0) FROM runtime_exposure_logs WHERE equipment_id={eq_id} AND exposure_type='hours' AND source_type='rams_demo_seed'"),
    ("schedule_classes", """SELECT rv.id, rv.label FROM reference_values rv
        JOIN reference_sets rs ON rs.id = rv.set_id AND rs.status = 'published'
        JOIN reference_domains d ON d.id = rs.domain_id
        WHERE UPPER(TRIM(d.code)) = 'ORG.SCHEDULE_CLASS' AND rv.is_active=1 LIMIT 5"""),
    ("recent_wos", f"SELECT id, code, closed_at FROM work_orders WHERE equipment_id={eq_id} ORDER BY id DESC LIMIT 8"),
    ("weibull_tables", "SELECT name FROM sqlite_master WHERE type='table' AND name LIKE '%weibull%'"),
    ("weibull_runs", f"SELECT id, beta, eta, adequate_sample FROM weibull_fit_results WHERE equipment_id={eq_id} ORDER BY id DESC LIMIT 3"),
    ("kpi_snapshots", f"SELECT id, period_start, period_end, mtbf, mttr, availability, failure_rate, event_count, data_quality_score FROM reliability_kpi_snapshots WHERE equipment_id={eq_id} ORDER BY id DESC LIMIT 8"),
    ("eligible_fe", f"SELECT id, source_type, eligible_flags_json FROM failure_events WHERE equipment_id={eq_id} LIMIT 5"),
]
for label, q in queries:
    print(f"--- {label} ---")
    print(cur.execute(q).fetchall())

con.close()
