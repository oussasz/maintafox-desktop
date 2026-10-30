//! Fixed content of the fictional company: Tellmétal Profilés du Nord (TPN), a steel and aluminium
//! profile manufacturer with two plants. Every name here is invented.

pub const COMPANY_CODE: &str = "TPN";
pub const COMPANY_NAME: &str = "Tellmétal Profilés du Nord";

/// One plant (org node of type site) with its maintenance cost centre.
pub struct Site {
    pub code: &'static str,
    pub name: &'static str,
    pub cost_center: &'static str,
    pub cost_center_name: &'static str,
}

pub const SITES: &[Site] = &[
    Site {
        code: "SET",
        name: "Usine de Sétif",
        cost_center: "CC-SET-MAINT",
        cost_center_name: "Maintenance Usine de Sétif",
    },
    Site {
        code: "BBA",
        name: "Usine de Bordj Bou Arréridj",
        cost_center: "CC-BBA-MAINT",
        cost_center_name: "Maintenance Usine de Bordj Bou Arréridj",
    },
];

/// Workshop (atelier) of a site.
pub struct Area {
    pub site: &'static str,
    pub code: &'static str,
    pub name: &'static str,
}

pub const AREAS: &[Area] = &[
    Area {
        site: "SET",
        code: "SET-EXT",
        name: "Atelier Extrusion aluminium",
    },
    Area {
        site: "SET",
        code: "SET-PRF",
        name: "Atelier Profilage acier",
    },
    Area {
        site: "SET",
        code: "SET-UTL",
        name: "Utilités Sétif",
    },
    Area {
        site: "BBA",
        code: "BBA-PRF",
        name: "Atelier Profilage à froid",
    },
    Area {
        site: "BBA",
        code: "BBA-TS",
        name: "Atelier Traitement de surface",
    },
    Area {
        site: "BBA",
        code: "BBA-UTL",
        name: "Utilités Bordj Bou Arréridj",
    },
];

/// Production line of a workshop.
pub struct Line {
    pub area: &'static str,
    pub code: &'static str,
    pub name: &'static str,
}

pub const LINES: &[Line] = &[
    Line {
        area: "SET-EXT",
        code: "SET-EXT-L1",
        name: "Ligne d'extrusion 1 (presse 1800 t)",
    },
    Line {
        area: "SET-EXT",
        code: "SET-EXT-L2",
        name: "Ligne d'extrusion 2 (presse 1450 t)",
    },
    Line {
        area: "SET-EXT",
        code: "SET-EXT-COM",
        name: "Équipements communs extrusion",
    },
    Line {
        area: "SET-PRF",
        code: "SET-PRF-L1",
        name: "Ligne de profilage PR-1",
    },
    Line {
        area: "SET-UTL",
        code: "SET-UTL-L1",
        name: "Air comprimé, refroidissement et énergie",
    },
    Line {
        area: "BBA-PRF",
        code: "BBA-PRF-L1",
        name: "Ligne de profilage 1 (bacs acier)",
    },
    Line {
        area: "BBA-PRF",
        code: "BBA-PRF-L2",
        name: "Ligne de profilage 2 (tubes soudés)",
    },
    Line {
        area: "BBA-TS",
        code: "BBA-TS-L1",
        name: "Ligne de thermolaquage",
    },
    Line {
        area: "BBA-UTL",
        code: "BBA-UTL-L1",
        name: "Air comprimé, levage et énergie",
    },
];

/// Equipment classes the company adds to the governed EQUIPMENT.CLASS list (the shipped list has
/// PUMP, MOTOR, VALVE, HEAT_EXCHANGER, COMPRESSOR, CONVEYOR, INSTRUMENTATION, VESSEL).
pub const EXTRA_CLASSES: &[(&str, &str)] = &[
    ("PRESS", "Presse"),
    ("FURNACE", "Four industriel"),
    ("SAW", "Scie et cisaille"),
    ("ROLL_FORMER", "Profileuse"),
    ("CRANE", "Pont roulant"),
    ("GEARBOX", "Réducteur"),
    ("WELDER", "Soudeuse"),
    ("PAINT_BOOTH", "Cabine de poudrage"),
    ("TRANSFORMER", "Transformateur"),
];

/// A maintained asset. `parent` is set for components (child assets of a machine).
pub struct Machine {
    pub line: &'static str,
    pub code: &'static str,
    pub name: &'static str,
    pub class: &'static str,
    pub criticality: &'static str,
    pub manufacturer: &'static str,
    pub model: &'static str,
    /// Replacement value in DZD.
    pub replacement_value: f64,
    /// Expected failures a year (drives the corrective history).
    pub failures_per_year: f64,
    /// Running hours on a working day.
    pub hours_per_day: f64,
    pub parent: Option<&'static str>,
    /// Whether work on it needs a lockout / tagout permit.
    pub loto: bool,
}

macro_rules! m {
    ($line:expr, $code:expr, $name:expr, $class:expr, $crit:expr, $mfr:expr, $model:expr, $value:expr, $rate:expr, $hours:expr, $loto:expr) => {
        Machine {
            line: $line,
            code: $code,
            name: $name,
            class: $class,
            criticality: $crit,
            manufacturer: $mfr,
            model: $model,
            replacement_value: $value,
            failures_per_year: $rate,
            hours_per_day: $hours,
            parent: None,
            loto: $loto,
        }
    };
}

macro_rules! c {
    ($parent:expr, $line:expr, $code:expr, $name:expr, $class:expr, $crit:expr, $mfr:expr, $model:expr, $value:expr, $rate:expr, $hours:expr, $loto:expr) => {
        Machine {
            line: $line,
            code: $code,
            name: $name,
            class: $class,
            criticality: $crit,
            manufacturer: $mfr,
            model: $model,
            replacement_value: $value,
            failures_per_year: $rate,
            hours_per_day: $hours,
            parent: Some($parent),
            loto: $loto,
        }
    };
}

pub const MACHINES: &[Machine] = &[
    // Sétif — extrusion line 1
    m!(
        "SET-EXT-L1",
        "SET-FP1",
        "Four de préchauffage billettes 1",
        "FURNACE",
        "B",
        "Thermoform Industrie",
        "FB-900",
        38_000_000.0,
        3.0,
        16.0,
        false
    ),
    m!(
        "SET-EXT-L1",
        "SET-PR1",
        "Presse d'extrusion 1800 t",
        "PRESS",
        "A",
        "Extrudal",
        "EXD-1800",
        420_000_000.0,
        5.5,
        16.0,
        true
    ),
    c!(
        "SET-PR1",
        "SET-EXT-L1",
        "SET-PR1-HYD",
        "Centrale hydraulique presse 1",
        "PUMP",
        "A",
        "Hydrasud",
        "CH-250",
        26_000_000.0,
        2.0,
        16.0,
        true
    ),
    c!(
        "SET-PR1",
        "SET-EXT-L1",
        "SET-PR1-MOT",
        "Moteur principal presse 1 (250 kW)",
        "MOTOR",
        "A",
        "Electromat",
        "MA-250",
        9_500_000.0,
        1.2,
        16.0,
        true
    ),
    m!(
        "SET-EXT-L1",
        "SET-SC1",
        "Scie à chaud 1",
        "SAW",
        "B",
        "Coupetech",
        "SCH-600",
        14_000_000.0,
        3.0,
        16.0,
        true
    ),
    m!(
        "SET-EXT-L1",
        "SET-TR1",
        "Table de refroidissement 1",
        "CONVEYOR",
        "C",
        "Convex",
        "TR-45",
        11_000_000.0,
        1.5,
        16.0,
        false
    ),
    m!(
        "SET-EXT-L1",
        "SET-ET1",
        "Banc d'étirage 1",
        "PRESS",
        "B",
        "Extrudal",
        "BE-30",
        16_000_000.0,
        2.5,
        16.0,
        false
    ),
    m!(
        "SET-EXT-L1",
        "SET-SF1",
        "Scie de finition 1",
        "SAW",
        "C",
        "Coupetech",
        "SF-500",
        7_500_000.0,
        1.5,
        16.0,
        false
    ),
    // Sétif — extrusion line 2
    m!(
        "SET-EXT-L2",
        "SET-FP2",
        "Four de préchauffage billettes 2",
        "FURNACE",
        "B",
        "Thermoform Industrie",
        "FB-700",
        32_000_000.0,
        3.0,
        16.0,
        false
    ),
    m!(
        "SET-EXT-L2",
        "SET-PR2",
        "Presse d'extrusion 1450 t",
        "PRESS",
        "A",
        "Extrudal",
        "EXD-1450",
        350_000_000.0,
        5.0,
        16.0,
        true
    ),
    c!(
        "SET-PR2",
        "SET-EXT-L2",
        "SET-PR2-HYD",
        "Centrale hydraulique presse 2",
        "PUMP",
        "A",
        "Hydrasud",
        "CH-200",
        22_000_000.0,
        2.0,
        16.0,
        true
    ),
    c!(
        "SET-PR2",
        "SET-EXT-L2",
        "SET-PR2-MOT",
        "Moteur principal presse 2 (200 kW)",
        "MOTOR",
        "A",
        "Electromat",
        "MA-200",
        8_000_000.0,
        1.2,
        16.0,
        true
    ),
    m!(
        "SET-EXT-L2",
        "SET-SC2",
        "Scie à chaud 2",
        "SAW",
        "B",
        "Coupetech",
        "SCH-500",
        12_000_000.0,
        3.0,
        16.0,
        true
    ),
    m!(
        "SET-EXT-L2",
        "SET-TR2",
        "Table de refroidissement 2",
        "CONVEYOR",
        "C",
        "Convex",
        "TR-40",
        10_000_000.0,
        1.5,
        16.0,
        false
    ),
    m!(
        "SET-EXT-L2",
        "SET-ET2",
        "Banc d'étirage 2",
        "PRESS",
        "B",
        "Extrudal",
        "BE-25",
        14_000_000.0,
        2.5,
        16.0,
        false
    ),
    m!(
        "SET-EXT-L2",
        "SET-SF2",
        "Scie de finition 2",
        "SAW",
        "C",
        "Coupetech",
        "SF-500",
        7_500_000.0,
        1.5,
        16.0,
        false
    ),
    // Sétif — common extrusion equipment
    m!(
        "SET-EXT-COM",
        "SET-FV1",
        "Four de vieillissement 1",
        "FURNACE",
        "B",
        "Thermoform Industrie",
        "FV-12",
        28_000_000.0,
        2.0,
        18.0,
        false
    ),
    m!(
        "SET-EXT-COM",
        "SET-FV2",
        "Four de vieillissement 2",
        "FURNACE",
        "B",
        "Thermoform Industrie",
        "FV-12",
        28_000_000.0,
        2.0,
        18.0,
        false
    ),
    m!(
        "SET-EXT-COM",
        "SET-FOR1",
        "Four à filières",
        "FURNACE",
        "C",
        "Thermoform Industrie",
        "FF-4",
        6_000_000.0,
        1.2,
        16.0,
        false
    ),
    m!(
        "SET-EXT-COM",
        "SET-PRT1",
        "Pont roulant 10 t extrusion",
        "CRANE",
        "B",
        "Levalg",
        "PR-10",
        18_000_000.0,
        2.0,
        12.0,
        true
    ),
    m!(
        "SET-EXT-COM",
        "SET-PRT2",
        "Pont roulant 5 t stock billettes",
        "CRANE",
        "C",
        "Levalg",
        "PR-5",
        12_000_000.0,
        1.2,
        10.0,
        true
    ),
    // Sétif — steel profiling line
    m!(
        "SET-PRF-L1",
        "SET-DER1",
        "Dérouleur bobines PR-1",
        "CONVEYOR",
        "C",
        "Profiform",
        "DR-10",
        9_000_000.0,
        1.5,
        16.0,
        false
    ),
    m!(
        "SET-PRF-L1",
        "SET-PRF1",
        "Profileuse 18 cages PR-1",
        "ROLL_FORMER",
        "A",
        "Profiform",
        "PF-18",
        160_000_000.0,
        5.0,
        16.0,
        true
    ),
    c!(
        "SET-PRF1",
        "SET-PRF-L1",
        "SET-PRF1-RED",
        "Réducteur principal PR-1",
        "GEARBOX",
        "A",
        "Transmeca",
        "RD-90",
        7_000_000.0,
        1.2,
        16.0,
        true
    ),
    c!(
        "SET-PRF1",
        "SET-PRF-L1",
        "SET-PRF1-MOT",
        "Moteur d'entraînement PR-1 (90 kW)",
        "MOTOR",
        "B",
        "Electromat",
        "MA-90",
        4_500_000.0,
        1.0,
        16.0,
        true
    ),
    m!(
        "SET-PRF-L1",
        "SET-CIS1",
        "Cisaille volante PR-1",
        "SAW",
        "B",
        "Coupetech",
        "CV-200",
        15_000_000.0,
        3.0,
        16.0,
        true
    ),
    m!(
        "SET-PRF-L1",
        "SET-PIN1",
        "Poinçonneuse PR-1",
        "PRESS",
        "B",
        "Profiform",
        "PN-60",
        13_000_000.0,
        2.5,
        16.0,
        true
    ),
    m!(
        "SET-PRF-L1",
        "SET-EMP1",
        "Empileur automatique PR-1",
        "CONVEYOR",
        "C",
        "Convex",
        "EA-6",
        8_000_000.0,
        1.5,
        16.0,
        false
    ),
    // Sétif — utilities
    m!(
        "SET-UTL-L1",
        "SET-CMP1",
        "Compresseur à vis 1 (90 kW)",
        "COMPRESSOR",
        "B",
        "Aircom",
        "AV-90",
        11_000_000.0,
        2.5,
        20.0,
        true
    ),
    m!(
        "SET-UTL-L1",
        "SET-CMP2",
        "Compresseur à vis 2 (90 kW)",
        "COMPRESSOR",
        "B",
        "Aircom",
        "AV-90",
        11_000_000.0,
        2.5,
        20.0,
        true
    ),
    m!(
        "SET-UTL-L1",
        "SET-SEC1",
        "Sécheur d'air frigorifique",
        "VESSEL",
        "C",
        "Aircom",
        "SF-15",
        2_500_000.0,
        1.0,
        20.0,
        false
    ),
    m!(
        "SET-UTL-L1",
        "SET-TRF1",
        "Tour de refroidissement",
        "HEAT_EXCHANGER",
        "B",
        "Hydrasud",
        "TF-800",
        19_000_000.0,
        2.0,
        20.0,
        false
    ),
    m!(
        "SET-UTL-L1",
        "SET-PMP1",
        "Pompe eau de refroidissement",
        "PUMP",
        "C",
        "Hydrasud",
        "PC-75",
        1_800_000.0,
        1.5,
        20.0,
        true
    ),
    m!(
        "SET-UTL-L1",
        "SET-TRA1",
        "Poste transformateur 30 kV",
        "TRANSFORMER",
        "B",
        "Electromat",
        "TR-2000",
        24_000_000.0,
        0.8,
        24.0,
        true
    ),
    // Bordj Bou Arréridj — profiling line 1
    m!(
        "BBA-PRF-L1",
        "BBA-DER1",
        "Dérouleur bobines L1",
        "CONVEYOR",
        "C",
        "Profiform",
        "DR-8",
        8_500_000.0,
        1.5,
        16.0,
        false
    ),
    m!(
        "BBA-PRF-L1",
        "BBA-PRF1",
        "Profileuse bacs acier L1",
        "ROLL_FORMER",
        "A",
        "Profiform",
        "PF-22",
        145_000_000.0,
        5.0,
        16.0,
        true
    ),
    c!(
        "BBA-PRF1",
        "BBA-PRF-L1",
        "BBA-PRF1-RED",
        "Réducteur principal L1",
        "GEARBOX",
        "A",
        "Transmeca",
        "RD-75",
        6_500_000.0,
        1.2,
        16.0,
        true
    ),
    c!(
        "BBA-PRF1",
        "BBA-PRF-L1",
        "BBA-PRF1-MOT",
        "Moteur d'entraînement L1 (75 kW)",
        "MOTOR",
        "B",
        "Electromat",
        "MA-75",
        4_000_000.0,
        1.0,
        16.0,
        true
    ),
    m!(
        "BBA-PRF-L1",
        "BBA-PIN1",
        "Poinçonneuse L1",
        "PRESS",
        "B",
        "Profiform",
        "PN-45",
        11_000_000.0,
        2.5,
        16.0,
        true
    ),
    m!(
        "BBA-PRF-L1",
        "BBA-CIS1",
        "Cisaille volante L1",
        "SAW",
        "B",
        "Coupetech",
        "CV-150",
        13_000_000.0,
        3.0,
        16.0,
        true
    ),
    m!(
        "BBA-PRF-L1",
        "BBA-EMP1",
        "Empileur automatique L1",
        "CONVEYOR",
        "C",
        "Convex",
        "EA-5",
        7_000_000.0,
        1.5,
        16.0,
        false
    ),
    // Bordj Bou Arréridj — profiling line 2 (welded tubes)
    m!(
        "BBA-PRF-L2",
        "BBA-DER2",
        "Dérouleur bobines L2",
        "CONVEYOR",
        "C",
        "Profiform",
        "DR-6",
        7_500_000.0,
        1.5,
        16.0,
        false
    ),
    m!(
        "BBA-PRF-L2",
        "BBA-PRF2",
        "Profileuse tubes L2",
        "ROLL_FORMER",
        "A",
        "Profiform",
        "PT-16",
        130_000_000.0,
        5.0,
        16.0,
        true
    ),
    c!(
        "BBA-PRF2",
        "BBA-PRF-L2",
        "BBA-PRF2-RED",
        "Réducteur principal L2",
        "GEARBOX",
        "A",
        "Transmeca",
        "RD-60",
        5_500_000.0,
        1.2,
        16.0,
        true
    ),
    c!(
        "BBA-PRF2",
        "BBA-PRF-L2",
        "BBA-PRF2-MOT",
        "Moteur d'entraînement L2 (55 kW)",
        "MOTOR",
        "B",
        "Electromat",
        "MA-55",
        3_200_000.0,
        1.0,
        16.0,
        true
    ),
    m!(
        "BBA-PRF-L2",
        "BBA-SOU2",
        "Soudeuse haute fréquence L2",
        "WELDER",
        "A",
        "Weldex",
        "HF-300",
        85_000_000.0,
        4.5,
        16.0,
        true
    ),
    m!(
        "BBA-PRF-L2",
        "BBA-CAL2",
        "Calibreuse tubes L2",
        "ROLL_FORMER",
        "B",
        "Profiform",
        "CT-8",
        18_000_000.0,
        2.5,
        16.0,
        false
    ),
    m!(
        "BBA-PRF-L2",
        "BBA-CIS2",
        "Scie volante tubes L2",
        "SAW",
        "B",
        "Coupetech",
        "SV-120",
        12_000_000.0,
        3.0,
        16.0,
        true
    ),
    m!(
        "BBA-PRF-L2",
        "BBA-EMP2",
        "Botteleuse tubes L2",
        "CONVEYOR",
        "C",
        "Convex",
        "BT-4",
        6_500_000.0,
        1.5,
        16.0,
        false
    ),
    // Bordj Bou Arréridj — powder coating line
    m!(
        "BBA-TS-L1",
        "BBA-PRE1",
        "Tunnel de prétraitement",
        "VESSEL",
        "B",
        "Surfatec",
        "TP-7",
        24_000_000.0,
        2.5,
        16.0,
        false
    ),
    m!(
        "BBA-TS-L1",
        "BBA-ETU1",
        "Étuve de séchage",
        "FURNACE",
        "C",
        "Thermoform Industrie",
        "ES-3",
        9_000_000.0,
        1.2,
        16.0,
        false
    ),
    m!(
        "BBA-TS-L1",
        "BBA-CAB1",
        "Cabine de poudrage automatique",
        "PAINT_BOOTH",
        "B",
        "Surfatec",
        "CP-12",
        42_000_000.0,
        3.0,
        16.0,
        false
    ),
    m!(
        "BBA-TS-L1",
        "BBA-POL1",
        "Four de polymérisation",
        "FURNACE",
        "A",
        "Thermoform Industrie",
        "FP-200",
        65_000_000.0,
        4.0,
        16.0,
        false
    ),
    m!(
        "BBA-TS-L1",
        "BBA-CNV1",
        "Convoyeur aérien thermolaquage",
        "CONVEYOR",
        "B",
        "Convex",
        "CA-300",
        21_000_000.0,
        3.0,
        16.0,
        true
    ),
    c!(
        "BBA-CNV1",
        "BBA-TS-L1",
        "BBA-CNV1-MOT",
        "Motoréducteur convoyeur aérien",
        "MOTOR",
        "C",
        "Transmeca",
        "MR-11",
        900_000.0,
        1.0,
        16.0,
        true
    ),
    // Bordj Bou Arréridj — utilities
    m!(
        "BBA-UTL-L1",
        "BBA-CMP1",
        "Compresseur à vis 1 (75 kW)",
        "COMPRESSOR",
        "B",
        "Aircom",
        "AV-75",
        9_500_000.0,
        2.5,
        20.0,
        true
    ),
    m!(
        "BBA-UTL-L1",
        "BBA-CMP2",
        "Compresseur à vis 2 (75 kW)",
        "COMPRESSOR",
        "B",
        "Aircom",
        "AV-75",
        9_500_000.0,
        2.5,
        20.0,
        true
    ),
    m!(
        "BBA-UTL-L1",
        "BBA-PRT1",
        "Pont roulant 8 t bobines",
        "CRANE",
        "B",
        "Levalg",
        "PR-8",
        16_000_000.0,
        2.0,
        12.0,
        true
    ),
    m!(
        "BBA-UTL-L1",
        "BBA-PMP1",
        "Pompe de relevage eaux de process",
        "PUMP",
        "C",
        "Hydrasud",
        "PR-30",
        1_200_000.0,
        1.2,
        20.0,
        true
    ),
    m!(
        "BBA-UTL-L1",
        "BBA-TRA1",
        "Poste transformateur 30 kV",
        "TRANSFORMER",
        "B",
        "Electromat",
        "TR-1600",
        21_000_000.0,
        0.8,
        24.0,
        true
    ),
];

/// Team (crew) of a site.
pub struct Team {
    pub site: &'static str,
    pub code: &'static str,
    pub name: &'static str,
    pub team_type: &'static str,
}

pub const TEAMS: &[Team] = &[
    Team {
        site: "SET",
        code: "SET-MECA",
        name: "Équipe mécanique Sétif",
        team_type: "maintenance",
    },
    Team {
        site: "SET",
        code: "SET-ELEC",
        name: "Équipe électricité Sétif",
        team_type: "maintenance",
    },
    Team {
        site: "SET",
        code: "SET-HSE",
        name: "Cellule HSE Sétif",
        team_type: "hse",
    },
    Team {
        site: "BBA",
        code: "BBA-MAINT",
        name: "Équipe maintenance Bordj Bou Arréridj",
        team_type: "maintenance",
    },
    Team {
        site: "BBA",
        code: "BBA-HSE",
        name: "Cellule HSE Bordj Bou Arréridj",
        team_type: "hse",
    },
];

/// What a person does in the simulation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Duty {
    /// Responsable maintenance: approves DIs and purchases, closes work orders.
    Manager,
    /// Chef d'équipe: screens DIs, plans and verifies work.
    Supervisor,
    /// Méthodes / planning.
    Planner,
    /// Technician with a login: can be the responsible of a work order and apply lockouts.
    LeadTechnician,
    Technician,
    Storekeeper,
    Hse,
    /// Chef de production: reports breakdowns.
    Production,
}

pub struct Person {
    pub site: &'static str,
    pub code: &'static str,
    pub full_name: &'static str,
    pub position: &'static str,
    pub team: &'static str,
    pub duty: Duty,
    /// Login created for this person (None = no account).
    pub username: Option<&'static str>,
    /// Seeded role granted to that login.
    pub role: &'static str,
    /// Hourly labour rate in DZD.
    pub rate: f64,
    pub skill: &'static str,
}

macro_rules! p {
    ($site:expr, $code:expr, $name:expr, $pos:expr, $team:expr, $duty:expr, $user:expr, $role:expr, $rate:expr, $skill:expr) => {
        Person {
            site: $site,
            code: $code,
            full_name: $name,
            position: $pos,
            team: $team,
            duty: $duty,
            username: $user,
            role: $role,
            rate: $rate,
            skill: $skill,
        }
    };
}

pub const PEOPLE: &[Person] = &[
    p!(
        "SET",
        "TPN-S01",
        "Karim Bensalem",
        "RESP_MAINT",
        "SET-MECA",
        Duty::Manager,
        Some("k.bensalem"),
        "Maintenance Supervisor",
        2_400.0,
        "MECH_GENERAL"
    ),
    p!(
        "SET",
        "TPN-S02",
        "Nadia Haddad",
        "PLANIF",
        "SET-MECA",
        Duty::Planner,
        Some("n.haddad"),
        "Planner/Scheduler",
        1_900.0,
        "MECH_GENERAL"
    ),
    p!(
        "SET",
        "TPN-S03",
        "Yacine Merabet",
        "CHEF_EQUIPE",
        "SET-MECA",
        Duty::Supervisor,
        Some("y.merabet"),
        "Maintenance Supervisor",
        1_800.0,
        "HYDRAULICS"
    ),
    p!(
        "SET",
        "TPN-S04",
        "Sofiane Khelifi",
        "TECH_MECA",
        "SET-MECA",
        Duty::LeadTechnician,
        Some("s.khelifi"),
        "Maintenance Technician",
        1_350.0,
        "MECH_GENERAL"
    ),
    p!(
        "SET",
        "TPN-S05",
        "Amine Rahmani",
        "TECH_MECA",
        "SET-MECA",
        Duty::Technician,
        None,
        "",
        1_200.0,
        "MECH_GENERAL"
    ),
    p!(
        "SET",
        "TPN-S06",
        "Rachid Mansouri",
        "TECH_MECA",
        "SET-MECA",
        Duty::Technician,
        None,
        "",
        1_250.0,
        "HYDRAULICS"
    ),
    p!(
        "SET",
        "TPN-S07",
        "Walid Cherif",
        "TECH_ELEC",
        "SET-ELEC",
        Duty::LeadTechnician,
        Some("w.cherif"),
        "Maintenance Technician",
        1_400.0,
        "ELEC_INDUSTRIAL"
    ),
    p!(
        "SET",
        "TPN-S08",
        "Hichem Zerrouki",
        "TECH_ELEC",
        "SET-ELEC",
        Duty::Technician,
        None,
        "",
        1_250.0,
        "ELEC_INDUSTRIAL"
    ),
    p!(
        "SET",
        "TPN-S09",
        "Bilal Hamidi",
        "TECH_ELEC",
        "SET-ELEC",
        Duty::Technician,
        None,
        "",
        1_200.0,
        "INSTRUMENTATION"
    ),
    p!(
        "SET",
        "TPN-S10",
        "Mourad Saidi",
        "MAGASINIER",
        "SET-MECA",
        Duty::Storekeeper,
        Some("m.saidi"),
        "Operator",
        1_000.0,
        "MECH_GENERAL"
    ),
    p!(
        "SET",
        "TPN-S11",
        "Samira Belkacem",
        "ANIM_HSE",
        "SET-HSE",
        Duty::Hse,
        Some("s.belkacem"),
        "Supervisor",
        1_600.0,
        "MECH_GENERAL"
    ),
    p!(
        "SET",
        "TPN-S12",
        "Mehdi Lounis",
        "CHEF_PROD",
        "SET-MECA",
        Duty::Production,
        Some("m.lounis"),
        "Operator",
        1_700.0,
        "MECH_GENERAL"
    ),
    p!(
        "BBA",
        "TPN-B01",
        "Djamel Ferhat",
        "RESP_MAINT",
        "BBA-MAINT",
        Duty::Manager,
        Some("d.ferhat"),
        "Maintenance Supervisor",
        2_300.0,
        "MECH_GENERAL"
    ),
    p!(
        "BBA",
        "TPN-B02",
        "Lotfi Benali",
        "CHEF_EQUIPE",
        "BBA-MAINT",
        Duty::Supervisor,
        Some("l.benali"),
        "Maintenance Supervisor",
        1_750.0,
        "ELEC_INDUSTRIAL"
    ),
    p!(
        "BBA",
        "TPN-B03",
        "Kamel Aissaoui",
        "TECH_MECA",
        "BBA-MAINT",
        Duty::LeadTechnician,
        Some("k.aissaoui"),
        "Maintenance Technician",
        1_350.0,
        "MECH_GENERAL"
    ),
    p!(
        "BBA",
        "TPN-B04",
        "Nabil Guerfi",
        "TECH_MECA",
        "BBA-MAINT",
        Duty::Technician,
        None,
        "",
        1_200.0,
        "WELDING"
    ),
    p!(
        "BBA",
        "TPN-B05",
        "Redouane Meziane",
        "TECH_ELEC",
        "BBA-MAINT",
        Duty::LeadTechnician,
        Some("r.meziane"),
        "Maintenance Technician",
        1_400.0,
        "ELEC_INDUSTRIAL"
    ),
    p!(
        "BBA",
        "TPN-B06",
        "Islem Bouzid",
        "TECH_ELEC",
        "BBA-MAINT",
        Duty::Technician,
        None,
        "",
        1_150.0,
        "INSTRUMENTATION"
    ),
    p!(
        "BBA",
        "TPN-B07",
        "Hamza Djebbar",
        "MAGASINIER",
        "BBA-MAINT",
        Duty::Storekeeper,
        Some("h.djebbar"),
        "Operator",
        1_000.0,
        "MECH_GENERAL"
    ),
    p!(
        "BBA",
        "TPN-B08",
        "Amel Kaci",
        "ANIM_HSE",
        "BBA-HSE",
        Duty::Hse,
        Some("a.kaci"),
        "Supervisor",
        1_550.0,
        "MECH_GENERAL"
    ),
    p!(
        "BBA",
        "TPN-B09",
        "Zineb Hamdi",
        "CHEF_PROD",
        "BBA-MAINT",
        Duty::Production,
        Some("z.hamdi"),
        "Operator",
        1_650.0,
        "MECH_GENERAL"
    ),
];

/// (code, name, PERSONNEL.POSITION_CATEGORIES code)
pub const POSITIONS: &[(&str, &str, &str)] = &[
    ("RESP_MAINT", "Responsable maintenance", "engineer"),
    ("PLANIF", "Planificatrice / méthodes", "planner"),
    ("CHEF_EQUIPE", "Chef d'équipe maintenance", "supervisor"),
    ("TECH_MECA", "Technicien de maintenance mécanique", "technician"),
    ("TECH_ELEC", "Technicien de maintenance électrique", "technician"),
    ("MAGASINIER", "Magasinier pièces de rechange", "storekeeper"),
    ("ANIM_HSE", "Animateur HSE", "hse"),
    ("CHEF_PROD", "Chef de production", "supervisor"),
];

/// Spare part: (code, name, unit, unit cost DZD, min, reorder point, max, family keyword for usage)
pub struct Article {
    pub code: &'static str,
    pub name: &'static str,
    pub unit: &'static str,
    pub unit_cost: f64,
    pub min: f64,
    pub reorder: f64,
    pub max: f64,
    /// Which classes consume it (empty = any).
    pub used_by: &'static [&'static str],
    pub critical: bool,
}

macro_rules! a {
    ($code:expr, $name:expr, $unit:expr, $cost:expr, $min:expr, $reorder:expr, $max:expr, $used:expr, $crit:expr) => {
        Article {
            code: $code,
            name: $name,
            unit: $unit,
            unit_cost: $cost,
            min: $min,
            reorder: $reorder,
            max: $max,
            used_by: $used,
            critical: $crit,
        }
    };
}

pub const ARTICLES: &[Article] = &[
    a!(
        "ROUL-6310",
        "Roulement à billes 6310 2RS",
        "U",
        4_800.0,
        4.0,
        6.0,
        20.0,
        &["MOTOR", "PUMP", "CONVEYOR", "SAW"],
        false
    ),
    a!(
        "ROUL-22220",
        "Roulement à rotule 22220 E",
        "U",
        38_000.0,
        2.0,
        3.0,
        8.0,
        &["ROLL_FORMER", "GEARBOX", "PRESS"],
        false
    ),
    a!(
        "ROUL-6205",
        "Roulement à billes 6205 2Z",
        "U",
        950.0,
        10.0,
        15.0,
        50.0,
        &["CONVEYOR", "MOTOR", "SAW"],
        false
    ),
    a!(
        "COUR-SPB",
        "Courroie trapézoïdale SPB 2000",
        "U",
        3_200.0,
        6.0,
        8.0,
        30.0,
        &["COMPRESSOR", "CONVEYOR", "SAW"],
        false
    ),
    a!(
        "JOINT-VER",
        "Jeu de joints vérin principal",
        "U",
        65_000.0,
        1.0,
        2.0,
        4.0,
        &["PRESS"],
        false
    ),
    a!(
        "JOINT-HYD",
        "Kit joints hydrauliques",
        "U",
        12_500.0,
        3.0,
        4.0,
        12.0,
        &["PRESS", "PUMP"],
        false
    ),
    a!(
        "FILT-HYD",
        "Cartouche filtre hydraulique 10 µm",
        "U",
        8_400.0,
        6.0,
        8.0,
        30.0,
        &["PRESS", "PUMP"],
        false
    ),
    a!(
        "FILT-AIR",
        "Filtre à air compresseur",
        "U",
        6_900.0,
        4.0,
        6.0,
        20.0,
        &["COMPRESSOR"],
        false
    ),
    a!(
        "FILT-HUI",
        "Filtre à huile compresseur",
        "U",
        5_200.0,
        4.0,
        6.0,
        20.0,
        &["COMPRESSOR"],
        false
    ),
    a!(
        "HUILE-HM46",
        "Huile hydraulique HM 46",
        "L",
        520.0,
        200.0,
        300.0,
        1_200.0,
        &["PRESS", "PUMP", "GEARBOX"],
        false
    ),
    a!(
        "HUILE-REDU",
        "Huile réducteur EP 220",
        "L",
        640.0,
        60.0,
        100.0,
        400.0,
        &["GEARBOX", "ROLL_FORMER"],
        false
    ),
    a!(
        "GRAISSE-EP2",
        "Graisse lithium EP2",
        "KG",
        900.0,
        20.0,
        30.0,
        120.0,
        &[],
        false
    ),
    a!(
        "CONT-LC1D",
        "Contacteur 3P 65 A 230 V",
        "U",
        14_500.0,
        3.0,
        4.0,
        12.0,
        &["MOTOR", "CONVEYOR", "COMPRESSOR", "CRANE"],
        false
    ),
    a!(
        "DISJ-MOT",
        "Disjoncteur moteur 25-40 A",
        "U",
        18_000.0,
        2.0,
        3.0,
        8.0,
        &["MOTOR", "PUMP", "CONVEYOR"],
        false
    ),
    a!(
        "CAPT-IND",
        "Détecteur inductif M18",
        "U",
        7_800.0,
        5.0,
        8.0,
        25.0,
        &["ROLL_FORMER", "SAW", "CONVEYOR", "PRESS"],
        false
    ),
    a!(
        "THERMO-K",
        "Thermocouple type K gainé",
        "U",
        9_600.0,
        4.0,
        6.0,
        20.0,
        &["FURNACE"],
        false
    ),
    a!(
        "RESIST-FOUR",
        "Résistance chauffante 6 kW",
        "U",
        42_000.0,
        2.0,
        3.0,
        8.0,
        &["FURNACE"],
        false
    ),
    a!(
        "BRULEUR-EL",
        "Électrode d'allumage brûleur gaz",
        "U",
        11_000.0,
        2.0,
        3.0,
        10.0,
        &["FURNACE"],
        false
    ),
    a!(
        "LAME-SCIE",
        "Lame de scie carbure Ø 550",
        "U",
        58_000.0,
        2.0,
        3.0,
        10.0,
        &["SAW"],
        false
    ),
    a!(
        "GALET-PRF",
        "Galet de profilage acier traité",
        "U",
        95_000.0,
        1.0,
        2.0,
        6.0,
        &["ROLL_FORMER"],
        true
    ),
    a!(
        "CHAINE-16B",
        "Chaîne à rouleaux 16B (5 m)",
        "U",
        21_000.0,
        2.0,
        3.0,
        8.0,
        &["CONVEYOR", "CRANE"],
        false
    ),
    a!(
        "CABLE-LEV",
        "Câble de levage Ø 14 (30 m)",
        "U",
        88_000.0,
        1.0,
        1.0,
        3.0,
        &["CRANE"],
        false
    ),
    a!(
        "BUSE-POUD",
        "Buse pistolet de poudrage",
        "U",
        7_400.0,
        6.0,
        10.0,
        30.0,
        &["PAINT_BOOTH"],
        false
    ),
    a!(
        "INDUCT-HF",
        "Inducteur soudeuse HF",
        "U",
        125_000.0,
        1.0,
        1.0,
        3.0,
        &["WELDER"],
        true
    ),
    a!(
        "POMPE-DOS",
        "Pompe doseuse prétraitement",
        "U",
        76_000.0,
        1.0,
        1.0,
        2.0,
        &["VESSEL"],
        false
    ),
    a!(
        "VAR-37KW",
        "Variateur de vitesse 37 kW",
        "U",
        260_000.0,
        1.0,
        1.0,
        2.0,
        &["MOTOR", "CONVEYOR", "COMPRESSOR"],
        true
    ),
    a!(
        "FUS-HT",
        "Fusible HT 30 kV 40 A",
        "U",
        23_000.0,
        3.0,
        3.0,
        9.0,
        &["TRANSFORMER"],
        false
    ),
    a!(
        "FLEX-HYD",
        "Flexible hydraulique HP DN 20",
        "U",
        9_800.0,
        4.0,
        6.0,
        16.0,
        &["PRESS", "PUMP"],
        false
    ),
];

/// (code, name, status, payment terms, lead time days)
pub const SUPPLIERS: &[(&str, &str, &str, &str, i64)] = &[
    ("FRN-ROULEST", "Roulements de l'Est SARL", "PREFERRED", "30J", 7),
    ("FRN-HYDRAMAG", "Hydramag Algérie", "APPROVED", "30J", 14),
    ("FRN-ELECSETIF", "Électro-Sétif Distribution", "PREFERRED", "30J", 5),
    ("FRN-LUBRINORD", "Lubrinord", "APPROVED", "COMPTANT", 3),
    ("FRN-OUTILPRO", "Outillage Pro Hauts-Plateaux", "APPROVED", "60J", 21),
    (
        "FRN-INDUSERV",
        "Induserv Pièces Industrielles",
        "UNDER_EVALUATION",
        "30J",
        30,
    ),
];

/// Annual maintenance budget per site and bucket (DZD), split evenly over the months.
pub const BUDGET_PER_SITE: &[(&str, f64)] = &[
    ("labor", 9_600_000.0),
    ("parts", 14_400_000.0),
    ("services", 4_800_000.0),
    ("other", 1_200_000.0),
];

/// Failure catalogue used by the corrective history: (mode, cause, symptom, mechanism hint,
/// root cause, corrective action, classes it applies to (empty = any), uses LOTO energy).
pub struct FailureStory {
    pub mode: &'static str,
    pub cause: &'static str,
    pub symptom: &'static str,
    pub title: &'static str,
    pub root_cause: &'static str,
    pub action: &'static str,
    pub classes: &'static [&'static str],
}

pub const FAILURE_STORIES: &[FailureStory] = &[
    FailureStory {
        mode: "VIBRATION",
        cause: "wear_and_tear",
        symptom: "vibration",
        title: "Vibrations anormales",
        root_cause: "Roulement usé (fin de vie).",
        action: "Remplacement du roulement et contrôle de l'alignement.",
        classes: &[
            "MOTOR",
            "PUMP",
            "GEARBOX",
            "ROLL_FORMER",
            "CONVEYOR",
            "COMPRESSOR",
            "SAW",
        ],
    },
    FailureStory {
        mode: "FUITE",
        cause: "wear_and_tear",
        symptom: "leakage",
        title: "Fuite d'huile hydraulique",
        root_cause: "Joints de vérin durcis par la température.",
        action: "Remplacement des joints et appoint d'huile.",
        classes: &["PRESS", "PUMP"],
    },
    FailureStory {
        mode: "SURCHAUFFE",
        cause: "maintenance_error",
        symptom: "overheating",
        title: "Surchauffe",
        root_cause: "Filtre colmaté, refroidissement insuffisant.",
        action: "Nettoyage du circuit et remplacement du filtre.",
        classes: &["COMPRESSOR", "PUMP", "MOTOR", "GEARBOX", "TRANSFORMER"],
    },
    FailureStory {
        mode: "PANNE_ELEC",
        cause: "wear_and_tear",
        symptom: "breakdown",
        title: "Défaut électrique, arrêt machine",
        root_cause: "Contacteur de puissance défaillant.",
        action: "Remplacement du contacteur et serrage des connexions.",
        classes: &[],
    },
    FailureStory {
        mode: "PANNE_ELEC",
        cause: "installation_error",
        symptom: "fail_to_start",
        title: "Refus de démarrage",
        root_cause: "Capteur de sécurité déréglé.",
        action: "Remplacement et réglage du détecteur.",
        classes: &["ROLL_FORMER", "SAW", "CONVEYOR", "PRESS", "PAINT_BOOTH"],
    },
    FailureStory {
        mode: "BRUIT",
        cause: "wear_and_tear",
        symptom: "noise",
        title: "Bruit anormal",
        root_cause: "Chaîne détendue et pignon usé.",
        action: "Remplacement de la chaîne, réglage de la tension.",
        classes: &["CONVEYOR", "CRANE"],
    },
    FailureStory {
        mode: "SURCHAUFFE",
        cause: "off_design_service",
        symptom: "overheating",
        title: "Dérive de température",
        root_cause: "Thermocouple défectueux, régulation instable.",
        action: "Remplacement du thermocouple et réétalonnage de la régulation.",
        classes: &["FURNACE"],
    },
    FailureStory {
        mode: "AUTRE",
        cause: "operating_error",
        symptom: "breakdown",
        title: "Lame cassée",
        root_cause: "Avance trop rapide sur profil épais.",
        action: "Remplacement de la lame, rappel des consignes à l'équipe.",
        classes: &["SAW"],
    },
    FailureStory {
        mode: "CORROSION",
        cause: "design_material",
        symptom: "leakage",
        title: "Fuite sur circuit d'eau",
        root_cause: "Corrosion de la tuyauterie.",
        action: "Remplacement du tronçon corrodé.",
        classes: &["HEAT_EXCHANGER", "VESSEL", "PUMP"],
    },
    FailureStory {
        mode: "PANNE_ELEC",
        cause: "wear_and_tear",
        symptom: "breakdown",
        title: "Défaut inducteur, soudure instable",
        root_cause: "Inducteur fissuré par fatigue thermique.",
        action: "Remplacement de l'inducteur et réglage de la puissance.",
        classes: &["WELDER"],
    },
];

/// Planned-work backlog open at the end of the history (overhauls and improvements prepared for
/// the next shutdown): (machine, type, title, expected hours, days before today it was raised).
pub const BACKLOG_PROJECTS: &[(&str, &str, &str, f64, i64)] = &[
    (
        "SET-PR1",
        "overhaul",
        "Révision générale presse d'extrusion 1800 t (arrêt annuel)",
        480.0,
        40,
    ),
    (
        "SET-PR2",
        "overhaul",
        "Révision générale presse d'extrusion 1450 t (arrêt annuel)",
        420.0,
        38,
    ),
    (
        "BBA-PRF1",
        "overhaul",
        "Révision des cages de la profileuse L1",
        300.0,
        30,
    ),
    (
        "BBA-PRF2",
        "overhaul",
        "Révision de la profileuse tubes L2 et de la soudeuse HF",
        320.0,
        26,
    ),
    (
        "BBA-POL1",
        "improvement",
        "Remplacement des brûleurs du four de polymérisation",
        220.0,
        21,
    ),
    (
        "SET-TR1",
        "improvement",
        "Remplacement des rouleaux de la table de refroidissement 1",
        140.0,
        18,
    ),
    (
        "SET-CMP1",
        "improvement",
        "Installation d'un variateur sur le compresseur 1",
        60.0,
        12,
    ),
    (
        "BBA-CNV1",
        "improvement",
        "Remplacement de la chaîne du convoyeur aérien",
        180.0,
        9,
    ),
    (
        "SET-TRA1",
        "overhaul",
        "Contrôle et analyse d'huile du transformateur 30 kV",
        40.0,
        6,
    ),
];
