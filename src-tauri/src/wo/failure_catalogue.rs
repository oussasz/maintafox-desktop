//! Failure-coding catalogue per equipment family and corrective remedies (ISO 14224 based).
//!
//! `WORK.FAILURE_MODES` ships 7 generic modes (see `reference::system_catalog_integrity`). This
//! module adds, additively, the common failure modes of the main industrial equipment families
//! and a new tenant-managed list `WORK.FAILURE_REMEDIES` (corrective actions).
//!
//! Decision: family modes stay in the flat list the close-out picker, the FMECA and the history
//! import all read (`parent_id` is not used by any consumer), so the family is carried by the
//! code prefix (`POMPE_CAVITATION`) and by the label (`Pompe — Cavitation`), which also groups
//! them alphabetically in every dropdown. Labels are French, `description` carries the English
//! term and `external_code` the closest ISO 14224 failure-mode code (Annex B, Tables B.6–B.15)
//! where one exists. Rows already present are never touched; seeded sync ids keep replicas
//! aligned; the startup integrity pass recreates everything after a tenant wipe.

use sea_orm::ConnectionTrait;

use crate::errors::AppResult;
use crate::wo::closeout_reference_catalog::{ensure_domain, ensure_value};

pub const DOMAIN_FAILURE_MODES: &str = "WORK.FAILURE_MODES";
pub const DOMAIN_FAILURE_REMEDIES: &str = "WORK.FAILURE_REMEDIES";

/// (code, French label, English term, ISO 14224 mode code or "")
type Row = (&'static str, &'static str, &'static str, &'static str);

const POMPE: &[Row] = &[
    ("POMPE_CAVITATION", "Pompe — Cavitation", "Pump cavitation", "LOO"),
    (
        "POMPE_DESALIGNEMENT",
        "Pompe — Désalignement pompe / moteur",
        "Pump/driver misalignment",
        "VIB",
    ),
    (
        "POMPE_GARNITURE_USURE",
        "Pompe — Usure de la garniture mécanique",
        "Mechanical seal wear",
        "ELP",
    ),
    (
        "POMPE_FUITE_PRESSE_ETOUPE",
        "Pompe — Fuite au presse-étoupe",
        "Gland packing leakage",
        "ELP",
    ),
    (
        "POMPE_ROULEMENT_GRIPPAGE",
        "Pompe — Grippage ou usure de roulement",
        "Bearing seizure / wear",
        "NOI",
    ),
    (
        "POMPE_ROUE_USURE",
        "Pompe — Usure ou érosion de la roue",
        "Impeller wear / erosion",
        "LOO",
    ),
    ("POMPE_DESAMORCAGE", "Pompe — Désamorçage", "Loss of prime", "LOO"),
    (
        "POMPE_COLMATAGE",
        "Pompe — Colmatage de la roue ou de l'aspiration",
        "Impeller / suction blockage",
        "PLU",
    ),
    (
        "POMPE_SURCHAUFFE",
        "Pompe — Échauffement du corps ou du palier",
        "Pump / bearing overheating",
        "OHE",
    ),
];

const MOTEUR: &[Row] = &[
    (
        "MOTEUR_ENROULEMENT_CC",
        "Moteur électrique — Court-circuit d'enroulement",
        "Winding short circuit",
        "BRD",
    ),
    (
        "MOTEUR_ISOLEMENT",
        "Moteur électrique — Défaut d'isolement à la terre",
        "Insulation / earth fault",
        "BRD",
    ),
    (
        "MOTEUR_ROULEMENT",
        "Moteur électrique — Défaillance de roulement",
        "Bearing failure",
        "NOI",
    ),
    (
        "MOTEUR_SURCHAUFFE",
        "Moteur électrique — Surchauffe (surcharge, ventilation)",
        "Overheating",
        "OHE",
    ),
    (
        "MOTEUR_DEMARRAGE",
        "Moteur électrique — Refus de démarrage",
        "Fail to start",
        "FTS",
    ),
    (
        "MOTEUR_VIBRATION",
        "Moteur électrique — Vibration (balourd, désalignement)",
        "Vibration",
        "VIB",
    ),
    (
        "MOTEUR_DECLENCHEMENT",
        "Moteur électrique — Déclenchement intempestif de la protection",
        "Spurious trip",
        "UST",
    ),
    (
        "MOTEUR_BARRES_ROTOR",
        "Moteur électrique — Barre rotorique cassée",
        "Broken rotor bar",
        "LOO",
    ),
];

const COMPRESSEUR: &[Row] = &[
    (
        "COMPRESSEUR_CLAPET",
        "Compresseur — Clapet ou soupape défaillant",
        "Valve failure",
        "LOO",
    ),
    (
        "COMPRESSEUR_SEGMENTS",
        "Compresseur — Usure des segments ou du cylindre",
        "Piston ring / cylinder wear",
        "LOO",
    ),
    (
        "COMPRESSEUR_FUITE_HUILE",
        "Compresseur — Fuite d'huile",
        "Oil leakage",
        "ELU",
    ),
    (
        "COMPRESSEUR_SURCHAUFFE",
        "Compresseur — Surchauffe de refoulement",
        "Discharge overheating",
        "OHE",
    ),
    (
        "COMPRESSEUR_PRESSION_BASSE",
        "Compresseur — Pression ou débit insuffisant",
        "Low pressure / output",
        "LOO",
    ),
    (
        "COMPRESSEUR_ROULEMENT",
        "Compresseur — Défaillance de roulement ou de palier",
        "Bearing failure",
        "NOI",
    ),
    (
        "COMPRESSEUR_SEPARATEUR",
        "Compresseur — Séparateur ou filtre colmaté",
        "Separator / filter clogged",
        "PLU",
    ),
    (
        "COMPRESSEUR_DEMARRAGE",
        "Compresseur — Refus de démarrage",
        "Fail to start",
        "FTS",
    ),
];

const VANNE: &[Row] = &[
    (
        "VANNE_FUITE_PRESSE_ETOUPE",
        "Vanne — Fuite à la tige / au presse-étoupe",
        "Stem / gland leakage",
        "ELP",
    ),
    (
        "VANNE_FUITE_SIEGE",
        "Vanne — Fuite au siège (passage en position fermée)",
        "Seat leakage in closed position",
        "LCP",
    ),
    (
        "VANNE_GRIPPAGE",
        "Vanne — Grippage de la tige ou de l'opercule",
        "Stem / disc sticking",
        "FTO",
    ),
    (
        "VANNE_NON_FERMETURE",
        "Vanne — Refus de fermeture",
        "Fail to close",
        "FTC",
    ),
    (
        "VANNE_ACTIONNEUR",
        "Vanne — Défaillance de l'actionneur ou du positionneur",
        "Actuator / positioner failure",
        "FTF",
    ),
    (
        "VANNE_EROSION_SIEGE",
        "Vanne — Érosion ou corrosion du siège et de l'obturateur",
        "Seat / trim erosion",
        "INL",
    ),
    (
        "VANNE_FUITE_CORPS",
        "Vanne — Fuite au corps ou aux brides",
        "Body / flange leakage",
        "ELP",
    ),
    (
        "VANNE_SOUPAPE_REGLAGE",
        "Vanne — Soupape de sécurité déréglée",
        "Relief valve out of setpoint",
        "DOP",
    ),
];

const ECHANGEUR: &[Row] = &[
    (
        "ECHANGEUR_ENCRASSEMENT",
        "Échangeur de chaleur — Encrassement",
        "Fouling",
        "LOO",
    ),
    (
        "ECHANGEUR_COLMATAGE_TUBES",
        "Échangeur de chaleur — Colmatage des tubes",
        "Tube plugging",
        "PLU",
    ),
    (
        "ECHANGEUR_FUITE_TUBES",
        "Échangeur de chaleur — Fuite de tube (mélange des fluides)",
        "Tube leak",
        "INL",
    ),
    (
        "ECHANGEUR_CORROSION",
        "Échangeur de chaleur — Corrosion",
        "Corrosion",
        "STD",
    ),
    (
        "ECHANGEUR_FUITE_JOINT",
        "Échangeur de chaleur — Fuite de joint (plaques / brides)",
        "Gasket leakage",
        "ELP",
    ),
    (
        "ECHANGEUR_PERTE_CHARGE",
        "Échangeur de chaleur — Perte de charge excessive",
        "High pressure drop",
        "PDE",
    ),
    (
        "ECHANGEUR_PERF_THERMIQUE",
        "Échangeur de chaleur — Performance thermique insuffisante",
        "Low thermal performance",
        "LOO",
    ),
];

const CONVOYEUR: &[Row] = &[
    (
        "CONVOYEUR_BANDE_USURE",
        "Convoyeur — Usure ou déchirure de la bande",
        "Belt wear / tear",
        "STD",
    ),
    (
        "CONVOYEUR_BANDE_DERIVE",
        "Convoyeur — Dérive ou désalignement de la bande",
        "Belt mistracking",
        "PDE",
    ),
    (
        "CONVOYEUR_RAMPE_ROULEAU",
        "Convoyeur — Rouleau ou tambour bloqué ou usé",
        "Roller / drum seized or worn",
        "NOI",
    ),
    (
        "CONVOYEUR_CHAINE",
        "Convoyeur — Allongement ou rupture de chaîne",
        "Chain elongation / breakage",
        "BRD",
    ),
    (
        "CONVOYEUR_BOURRAGE",
        "Convoyeur — Bourrage de produit",
        "Material jam",
        "PLU",
    ),
    (
        "CONVOYEUR_ENTRAINEMENT",
        "Convoyeur — Défaillance de l'entraînement (moteur / réducteur)",
        "Drive failure",
        "FTS",
    ),
    (
        "CONVOYEUR_PATINAGE",
        "Convoyeur — Patinage de la bande sur le tambour",
        "Belt slip",
        "LOO",
    ),
];

const REDUCTEUR: &[Row] = &[
    (
        "REDUCTEUR_DENTURE",
        "Réducteur — Usure ou casse de denture",
        "Gear tooth wear / breakage",
        "NOI",
    ),
    (
        "REDUCTEUR_ROULEMENT",
        "Réducteur — Défaillance de roulement",
        "Bearing failure",
        "NOI",
    ),
    (
        "REDUCTEUR_FUITE_HUILE",
        "Réducteur — Fuite d'huile (joint à lèvres, carter)",
        "Oil leakage",
        "ELU",
    ),
    (
        "REDUCTEUR_SURCHAUFFE",
        "Réducteur — Surchauffe de l'huile ou du carter",
        "Overheating",
        "OHE",
    ),
    (
        "REDUCTEUR_HUILE_DEGRADEE",
        "Réducteur — Huile dégradée ou contaminée",
        "Degraded / contaminated oil",
        "SER",
    ),
    (
        "REDUCTEUR_VIBRATION",
        "Réducteur — Vibration anormale",
        "Abnormal vibration",
        "VIB",
    ),
    (
        "REDUCTEUR_ARBRE",
        "Réducteur — Rupture ou usure d'arbre / clavette",
        "Shaft / key breakage or wear",
        "BRD",
    ),
];

const INSTRUMENT: &[Row] = &[
    (
        "INSTRUMENT_DERIVE",
        "Instrument / capteur — Dérive de mesure",
        "Measurement drift",
        "PDE",
    ),
    (
        "INSTRUMENT_SIGNAL_ABSENT",
        "Instrument / capteur — Absence de signal",
        "No signal",
        "AIR",
    ),
    (
        "INSTRUMENT_SIGNAL_ERRATIQUE",
        "Instrument / capteur — Signal erratique ou bruité",
        "Erratic signal",
        "ERO",
    ),
    (
        "INSTRUMENT_BLOCAGE_VALEUR",
        "Instrument / capteur — Valeur figée",
        "Frozen reading",
        "AIR",
    ),
    (
        "INSTRUMENT_PRISE_BOUCHEE",
        "Instrument / capteur — Prise d'impulsion bouchée",
        "Impulse line plugged",
        "PLU",
    ),
    (
        "INSTRUMENT_CABLAGE",
        "Instrument / capteur — Défaut de câblage ou de connexion",
        "Wiring / connection fault",
        "AIR",
    ),
    (
        "INSTRUMENT_ALIMENTATION",
        "Instrument / capteur — Défaut d'alimentation",
        "Power supply fault",
        "FOV",
    ),
];

const VENTILATEUR: &[Row] = &[
    (
        "VENTILATEUR_BALOURD",
        "Ventilateur — Balourd (encrassement ou usure de la turbine)",
        "Impeller unbalance",
        "VIB",
    ),
    (
        "VENTILATEUR_ROULEMENT",
        "Ventilateur — Défaillance de roulement",
        "Bearing failure",
        "NOI",
    ),
    (
        "VENTILATEUR_COURROIE",
        "Ventilateur — Courroie usée ou détendue",
        "Belt wear / slack",
        "LOO",
    ),
    (
        "VENTILATEUR_DEBIT",
        "Ventilateur — Débit d'air insuffisant",
        "Low airflow",
        "LOO",
    ),
    (
        "VENTILATEUR_ENCRASSEMENT",
        "Ventilateur — Encrassement des pales ou des filtres",
        "Blade / filter fouling",
        "PLU",
    ),
    (
        "VENTILATEUR_PALE_RUPTURE",
        "Ventilateur — Fissure ou rupture de pale",
        "Blade crack / breakage",
        "STD",
    ),
    (
        "VENTILATEUR_MOTEUR",
        "Ventilateur — Défaillance du moteur d'entraînement",
        "Drive motor failure",
        "FTS",
    ),
];

const ARMOIRE: &[Row] = &[
    (
        "ARMOIRE_CONTACTEUR",
        "Armoire électrique — Contacteur ou relais collé / usé",
        "Contactor / relay failure",
        "FTF",
    ),
    (
        "ARMOIRE_DISJONCTEUR",
        "Armoire électrique — Déclenchement intempestif du disjoncteur",
        "Spurious breaker trip",
        "UST",
    ),
    (
        "ARMOIRE_CONNEXION",
        "Armoire électrique — Connexion desserrée ou échauffée",
        "Loose / overheated connection",
        "OHE",
    ),
    (
        "ARMOIRE_FUSIBLE",
        "Armoire électrique — Fusible fondu",
        "Blown fuse",
        "FTF",
    ),
    (
        "ARMOIRE_VARIATEUR",
        "Armoire électrique — Défaut de variateur de vitesse",
        "Variable speed drive fault",
        "FTF",
    ),
    (
        "ARMOIRE_AUTOMATE",
        "Armoire électrique — Défaut d'automate ou de module d'E/S",
        "PLC / I/O module fault",
        "FTF",
    ),
    (
        "ARMOIRE_HUMIDITE",
        "Armoire électrique — Humidité, poussière ou surchauffe interne",
        "Moisture / dust / internal overheating",
        "OHE",
    ),
    (
        "ARMOIRE_ALIMENTATION",
        "Armoire électrique — Défaut d'alimentation (tension, phase manquante)",
        "Supply fault / missing phase",
        "FOV",
    ),
];

const CHAUDIERE: &[Row] = &[
    (
        "CHAUDIERE_BRULEUR",
        "Chaudière — Défaut de brûleur ou d'allumage",
        "Burner / ignition failure",
        "FTS",
    ),
    (
        "CHAUDIERE_FUITE_TUBES",
        "Chaudière — Fuite de tube de fumée ou d'eau",
        "Tube leakage",
        "ELP",
    ),
    (
        "CHAUDIERE_ENTARTRAGE",
        "Chaudière — Entartrage ou dépôts côté eau",
        "Scaling / deposits",
        "LOO",
    ),
    (
        "CHAUDIERE_NIVEAU",
        "Chaudière — Défaut de régulation de niveau d'eau",
        "Water level control fault",
        "PDE",
    ),
    (
        "CHAUDIERE_SOUPAPE",
        "Chaudière — Soupape de sécurité qui fuit ou déréglée",
        "Safety valve leakage / out of setpoint",
        "LCP",
    ),
    (
        "CHAUDIERE_REFRACTAIRE",
        "Chaudière — Dégradation du réfractaire ou de l'isolation",
        "Refractory / insulation degradation",
        "STD",
    ),
    (
        "CHAUDIERE_PRESSION",
        "Chaudière — Pression ou production de vapeur insuffisante",
        "Low steam pressure / output",
        "LOO",
    ),
];

const FRIGO: &[Row] = &[
    (
        "FRIGO_FUITE_FLUIDE",
        "Groupe frigorifique — Fuite de fluide frigorigène",
        "Refrigerant leakage",
        "ELU",
    ),
    (
        "FRIGO_COMPRESSEUR",
        "Groupe frigorifique — Défaillance du compresseur",
        "Compressor failure",
        "BRD",
    ),
    (
        "FRIGO_CONDENSEUR",
        "Groupe frigorifique — Condenseur encrassé ou ventilateur défaillant",
        "Condenser fouling / fan failure",
        "OHE",
    ),
    (
        "FRIGO_DETENDEUR",
        "Groupe frigorifique — Détendeur déréglé ou bloqué",
        "Expansion valve fault",
        "PDE",
    ),
    (
        "FRIGO_GIVRAGE",
        "Groupe frigorifique — Givrage de l'évaporateur",
        "Evaporator icing",
        "LOO",
    ),
    (
        "FRIGO_PRESSOSTAT",
        "Groupe frigorifique — Déclenchement HP / BP (pressostat)",
        "HP / LP trip",
        "UST",
    ),
    (
        "FRIGO_PUISSANCE",
        "Groupe frigorifique — Puissance frigorifique insuffisante",
        "Insufficient cooling capacity",
        "LOO",
    ),
];

/// Family lists in display order; each family starts a block of 100 in `sort_order` (the generic
/// codes keep 1–99).
const FAMILIES: &[&[Row]] = &[
    POMPE,
    MOTEUR,
    COMPRESSEUR,
    VANNE,
    ECHANGEUR,
    CONVOYEUR,
    REDUCTEUR,
    INSTRUMENT,
    VENTILATEUR,
    ARMOIRE,
    CHAUDIERE,
    FRIGO,
];

/// Corrective remedies (code, French label, English term). Generic starter list; each plant
/// extends or retires values through the reference manager.
const REMEDIES: &[(&str, &str, &str)] = &[
    (
        "REM_REMPLACEMENT_ROULEMENT",
        "Remplacement de roulement",
        "Bearing replacement",
    ),
    (
        "REM_REMPLACEMENT_GARNITURE",
        "Remplacement de la garniture mécanique",
        "Mechanical seal replacement",
    ),
    (
        "REM_REGLAGE_PRESSE_ETOUPE",
        "Resserrage ou remplacement de la tresse du presse-étoupe",
        "Gland tightening / repacking",
    ),
    (
        "REM_REMPLACEMENT_JOINT",
        "Remplacement de joint",
        "Gasket / seal replacement",
    ),
    ("REM_REALIGNEMENT", "Réalignement des arbres", "Shaft realignment"),
    ("REM_EQUILIBRAGE", "Équilibrage du rotor", "Rotor balancing"),
    (
        "REM_RESSERRAGE",
        "Resserrage de la boulonnerie ou des connexions",
        "Re-tightening of bolts / connections",
    ),
    ("REM_NETTOYAGE", "Nettoyage / décolmatage", "Cleaning / unclogging"),
    (
        "REM_GRAISSAGE",
        "Graissage ou appoint de lubrifiant",
        "Lubrication / grease top-up",
    ),
    ("REM_VIDANGE_HUILE", "Vidange et remplacement de l'huile", "Oil change"),
    (
        "REM_REMPLACEMENT_FILTRE",
        "Remplacement de filtre",
        "Filter replacement",
    ),
    (
        "REM_REMPLACEMENT_COURROIE",
        "Remplacement de courroie",
        "Belt replacement",
    ),
    (
        "REM_REMPLACEMENT_CHAINE",
        "Remplacement ou réglage de chaîne",
        "Chain replacement / adjustment",
    ),
    (
        "REM_REMPLACEMENT_ROUE",
        "Remplacement de la roue / de la turbine",
        "Impeller replacement",
    ),
    (
        "REM_REMPLACEMENT_MOTEUR",
        "Remplacement ou rebobinage du moteur",
        "Motor replacement / rewinding",
    ),
    (
        "REM_REMPLACEMENT_COMPOSANT_ELEC",
        "Remplacement d'un composant électrique (contacteur, relais, fusible)",
        "Electrical component replacement",
    ),
    (
        "REM_REPARATION_CABLAGE",
        "Réparation du câblage ou de la connexion",
        "Wiring / connection repair",
    ),
    (
        "REM_REMPLACEMENT_CAPTEUR",
        "Remplacement de capteur ou d'instrument",
        "Sensor / instrument replacement",
    ),
    ("REM_ETALONNAGE", "Réétalonnage / réglage", "Recalibration / adjustment"),
    (
        "REM_REMPLACEMENT_VANNE",
        "Remplacement ou rénovation de vanne (siège, obturateur)",
        "Valve replacement / refurbishment",
    ),
    (
        "REM_REMPLACEMENT_TUBES",
        "Remplacement ou bouchage de tubes",
        "Tube replacement / plugging",
    ),
    ("REM_REPARATION_SOUDURE", "Réparation par soudure", "Weld repair"),
    (
        "REM_RETOUR_FABRICANT",
        "Renvoi en réparation chez le fabricant ou un atelier",
        "Return to manufacturer / workshop",
    ),
    (
        "REM_REINITIALISATION",
        "Réinitialisation / redémarrage de l'équipement",
        "Reset / restart",
    ),
    (
        "REM_MISE_A_JOUR_PROGRAMME",
        "Mise à jour ou rechargement du programme",
        "Software update / reload",
    ),
    (
        "REM_AJUSTEMENT_PROCEDE",
        "Ajustement des conditions d'exploitation",
        "Operating conditions adjustment",
    ),
    (
        "REM_FORMATION",
        "Formation / rappel de consigne à l'opérateur",
        "Operator training / instruction reminder",
    ),
    (
        "REM_MODIFICATION",
        "Modification ou amélioration de conception",
        "Design modification / improvement",
    ),
    (
        "REM_SURVEILLANCE",
        "Mise sous surveillance (aucune réparation immédiate)",
        "Monitoring only (no immediate repair)",
    ),
    (
        "REM_AUCUNE",
        "Aucune action (pas de défaut constaté)",
        "No action (no fault found)",
    ),
];

/// Ensure the per-family failure modes and the remedies list exist (additive, idempotent).
/// Must run after the generic `WORK.FAILURE_MODES` seed (which creates the domain and its set).
pub async fn ensure_failure_catalogue(conn: &impl ConnectionTrait) -> AppResult<()> {
    for (f, rows) in FAMILIES.iter().enumerate() {
        for (i, (code, label, en, iso)) in rows.iter().enumerate() {
            let sort_order = 100 * (f as i64 + 1) + i as i64 + 1;
            let iso = Some(*iso).filter(|s| !s.is_empty());
            ensure_value(conn, DOMAIN_FAILURE_MODES, code, label, Some(en), iso, sort_order).await?;
        }
    }
    ensure_domain(conn, DOMAIN_FAILURE_REMEDIES, "Remèdes / actions correctives").await?;
    for (i, (code, label, en)) in REMEDIES.iter().enumerate() {
        ensure_value(conn, DOMAIN_FAILURE_REMEDIES, code, label, Some(en), None, i as i64 + 1).await?;
    }
    Ok(())
}
