# Seed Data Reference

Source: `src-tauri/src/db/seeder.rs` — version controlled alongside the code.

## System Seed Schema Version: 1

Recorded in `system_config` table under key `seed_schema_version`.

## Governed Domains (18 system domains)

| Domain Key | Display Name (FR) | Type | Extensible | Ordered | Values |
|------------|-------------------|------|------------|---------|--------|
| `equipment.criticality` | Criticite equipement | system | No | Yes | 4 |
| `equipment.lifecycle_status` | Statut cycle de vie equipement | system | No | No | 6 |
| `equipment.hierarchy_relationship` | Type de relation hierarchique | system | No | No | 4 |
| `intervention_request.type` | Type de demande d'intervention | tenant | Yes | No | 3 |
| `intervention_request.urgency` | Urgence DI | system | No | Yes | 4 |
| `intervention_request.status` | Statut DI | system | No | No | 7 |
| `work_order.type` | Type d'OT | tenant | Yes | No | 5 |
| `work_order.status` | Statut OT | system | No | No | 8 |
| `work_order.priority` | Priorite OT | system | No | Yes | 4 |
| `failure.mode` | Mode de defaillance | tenant | Yes | No | 7 |
| `failure.cause` | Cause de defaillance | tenant | Yes | No | 6 |
| `work_order.closure_reason` | Motif de cloture OT | tenant | Yes | No | 4 |
| `personnel.skill_proficiency` | Niveau de competence | system | No | Yes | 5 |
| `personnel.contract_type` | Type de contrat | tenant | Yes | No | 5 |
| `inventory.unit_of_measure` | Unite de mesure stock | tenant | Yes | No | 8 |
| `inventory.movement_type` | Type de mouvement stock | system | No | No | 5 |
| `org.responsibility_type` | Type de responsabilite org. | system | No | Yes | 5 |
| `permit.type` | Type de permis de travail | tenant | Yes | No | 5 |

**Total: 18 domains, 92 values**

## Value Breakdown by Domain

### equipment.criticality (4 values)
| Code | Label (FR) | Label (EN) | Color | System |
|------|-----------|-----------|-------|--------|
| CRITIQUE | Critique | Critical | #dc3545 | Yes |
| IMPORTANT | Important | Important | #ffc107 | Yes |
| STANDARD | Standard | Standard | #0dcaf0 | Yes |
| NON_CRITIQUE | Non-critique | Non-critical | #198754 | Yes |

### equipment.lifecycle_status (6 values)
| Code | Label (FR) | Label (EN) | Color | System |
|------|-----------|-----------|-------|--------|
| ACTIVE_IN_SERVICE | En service | In Service | #198754 | Yes |
| IN_STOCK | En stock | In Stock | #0dcaf0 | Yes |
| UNDER_MAINTENANCE | En maintenance | Under Maintenance | #ffc107 | Yes |
| DECOMMISSIONED | Mis hors service | Decommissioned | #6c757d | Yes |
| SCRAPPED | Mis au rebut | Scrapped | #dc3545 | Yes |
| SPARE | Piece de rechange | Spare | #6c757d | Yes |

### equipment.hierarchy_relationship (4 values)
| Code | Label (FR) | Label (EN) | System |
|------|-----------|-----------|--------|
| PARENT_CHILD | Parent - Enfant | Parent - Child | Yes |
| INSTALLED_IN | Installe dans | Installed In | Yes |
| DRIVES | Entraine | Drives | Yes |
| FEEDS | Alimente | Feeds | Yes |

### intervention_request.type (3 values)
| Code | Label (FR) | Label (EN) | System |
|------|-----------|-----------|--------|
| CORRECTIVE | Corrective | Corrective | Yes |
| SIGNALEMENT | Signalement | Observation | Yes |
| AMELIORATION | Amelioration | Improvement | No |

### intervention_request.urgency (4 values)
| Code | Label (FR) | Label (EN) | Color | System |
|------|-----------|-----------|-------|--------|
| IMMEDIATE | Immediate | Immediate | #dc3545 | Yes |
| URGENT | Urgente | Urgent | #ffc107 | Yes |
| NORMALE | Normale | Normal | #198754 | Yes |
| PLANIFIEE | Planifiee | Planned | #0dcaf0 | Yes |

### intervention_request.status (7 values)
| Code | Label (FR) | Label (EN) | Color | System |
|------|-----------|-----------|-------|--------|
| DRAFT | Brouillon | Draft | #6c757d | Yes |
| SUBMITTED | Soumise | Submitted | #0dcaf0 | Yes |
| ACKNOWLEDGED | Accusee | Acknowledged | #ffc107 | Yes |
| IN_PROGRESS | En cours | In Progress | #003d8f | Yes |
| COMPLETED | Cloturee | Completed | #198754 | Yes |
| REJECTED | Rejetee | Rejected | #dc3545 | Yes |
| CANCELLED | Annulee | Cancelled | #6c757d | Yes |

### work_order.type (5 values)
| Code | Label (FR) | Label (EN) | System |
|------|-----------|-----------|--------|
| CORRECTIVE | Corrective | Corrective | Yes |
| PREVENTIVE | Preventive | Preventive | Yes |
| PREDICTIVE | Predictive | Predictive | Yes |
| AMELIORATIVE | Ameliorative | Improvement | Yes |
| INSPECTION | Inspection | Inspection | Yes |

### work_order.status (8 values)
| Code | Label (FR) | Label (EN) | Color | System |
|------|-----------|-----------|-------|--------|
| DRAFT | Brouillon | Draft | #6c757d | Yes |
| PLANNED | Planifie | Planned | #0dcaf0 | Yes |
| RELEASED | Lance | Released | #003d8f | Yes |
| IN_PROGRESS | En cours | In Progress | #ffc107 | Yes |
| ON_HOLD | En attente | On Hold | #f0a500 | Yes |
| COMPLETED | Termine | Completed | #198754 | Yes |
| CLOSED | Cloture | Closed | #6c757d | Yes |
| CANCELLED | Annule | Cancelled | #dc3545 | Yes |

### work_order.priority (4 values)
| Code | Label (FR) | Label (EN) | Color | System |
|------|-----------|-----------|-------|--------|
| P1_CRITICAL | P1 - Critique | P1 - Critical | #dc3545 | Yes |
| P2_HIGH | P2 - Haute | P2 - High | #ffc107 | Yes |
| P3_MEDIUM | P3 - Moyenne | P3 - Medium | #0dcaf0 | Yes |
| P4_LOW | P4 - Basse | P4 - Low | #198754 | Yes |

### failure.mode (7 values)
| Code | Label (FR) | Label (EN) | System |
|------|-----------|-----------|--------|
| VIBRATION | Vibration | Vibration | Yes |
| CORROSION | Corrosion | Corrosion | Yes |
| BRUIT | Bruit anormal | Abnormal Noise | Yes |
| FUITE | Fuite | Leak | Yes |
| SURCHAUFFE | Surchauffe | Overheating | Yes |
| PANNE_ELEC | Panne electrique | Electrical Fault | Yes |
| AUTRE | Autre | Other | Yes |

### failure.cause (6 values)
| Code | Label (FR) | Label (EN) | System |
|------|-----------|-----------|--------|
| USURE_NORMALE | Usure normale | Normal Wear | Yes |
| MAUVAIS_USAGE | Mauvais usage | Misuse | Yes |
| DEFAUT_ENTRETIEN | Defaut d'entretien | Maintenance Defect | Yes |
| DEFAUT_INSTALL | Defaut d'installation | Installation Defect | Yes |
| DEFAUT_MATERIEL | Defaut materiel | Material Defect | Yes |
| INCONNU | Inconnu | Unknown | Yes |

### work_order.closure_reason (4 values)
| Code | Label (FR) | Label (EN) | Color | System |
|------|-----------|-----------|-------|--------|
| REPARE | Repare | Repaired | #198754 | Yes |
| REPORTE | Reporte | Deferred | #ffc107 | Yes |
| NON_NECESSAIRE | Non necessaire | Not Required | #6c757d | Yes |
| REMPLACE | Remplace | Replaced | #0dcaf0 | Yes |

### personnel.skill_proficiency (5 values)
| Code | Label (FR) | Label (EN) | System |
|------|-----------|-----------|--------|
| NIVEAU_1 | Niveau 1 - Notions | Level 1 - Awareness | Yes |
| NIVEAU_2 | Niveau 2 - Applique | Level 2 - Applied | Yes |
| NIVEAU_3 | Niveau 3 - Maitrise | Level 3 - Proficient | Yes |
| NIVEAU_4 | Niveau 4 - Expert | Level 4 - Expert | Yes |
| NIVEAU_5 | Niveau 5 - Maitre formateur | Level 5 - Master Trainer | Yes |

### personnel.contract_type (5 values)
| Code | Label (FR) | Label (EN) | System |
|------|-----------|-----------|--------|
| CDI | CDI | Permanent | Yes |
| CDD | CDD | Fixed-term | Yes |
| INTERIMAIRE | Interimaire | Temporary Agency | Yes |
| PRESTATAIRE | Prestataire externe | Contractor | Yes |
| STAGIAIRE | Stagiaire | Intern | No |

### inventory.unit_of_measure (8 values)
| Code | Label (FR) | Label (EN) | System |
|------|-----------|-----------|--------|
| U | Unite | Unit | Yes |
| KG | kg | kg | Yes |
| L | L | L | Yes |
| M | m | m | Yes |
| M2 | m2 | m2 | Yes |
| BOX | Boite | Box | Yes |
| ROUL | Rouleau | Roll | Yes |
| PAIRE | Paire | Pair | Yes |

### inventory.movement_type (5 values)
| Code | Label (FR) | Label (EN) | System |
|------|-----------|-----------|--------|
| SORTIE_OT | Sortie sur OT | Issue to WO | Yes |
| ENTREE_ACHAT | Entree achat | Purchase Receipt | Yes |
| RETOUR_OT | Retour d'OT | Return from WO | Yes |
| AJUSTEMENT | Ajustement inventaire | Inventory Adjustment | Yes |
| INVENTAIRE | Saisie inventaire | Stock Count Entry | Yes |

### org.responsibility_type (5 values)
| Code | Label (FR) | Label (EN) | System |
|------|-----------|-----------|--------|
| MAINTENANCE_OWNER | Responsable maintenance | Maintenance Owner | Yes |
| PRODUCTION_OWNER | Responsable production | Production Owner | Yes |
| HSE_OWNER | Responsable HSE | HSE Owner | Yes |
| PLANNER | Planificateur | Planner | Yes |
| APPROVER | Approbateur | Approver | Yes |

### permit.type (5 values)
| Code | Label (FR) | Label (EN) | System |
|------|-----------|-----------|--------|
| PERMIS_FEU | Permis de feu | Hot Work Permit | Yes |
| PERMIS_ELECTRIQUE | Permis electrique | Electrical Permit | Yes |
| PERMIS_HAUTEUR | Travail en hauteur | Work at Height | Yes |
| PERMIS_ESPACE | Espace confine | Confined Space | Yes |
| PERMIS_GENERAL | Permis general | General Permit | Yes |

## Protected Values

Values with `is_system = 1` cannot be deleted via the Lookup Manager UI. They can be
deactivated (set `is_active = 0`) but their codes remain reserved and are used by the
application's business logic for conditional rendering and workflow routing.

## Adding New System Values in a Future Release

1. Add the new `seed_value()` call to `seeder.rs`
2. Increment `SEED_SCHEMA_VERSION`
3. The seeder uses `INSERT OR IGNORE` — existing values are untouched
4. Update this document to reflect the new values
5. Add a migration integrity test assertion if the value is load-bearing
