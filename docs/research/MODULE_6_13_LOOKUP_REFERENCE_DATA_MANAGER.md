# Module 6.13 Lookup and Reference Data Manager Research Brief

## 1. Research Position

This module should not be treated as a generic CRUD editor for dropdown values.

In a serious maintenance platform, reference data is the semantic control layer that determines whether records can be searched consistently, analyzed correctly, integrated safely, and compared over time.

The most important distinction is between:

- convenience lookup lists that help users enter data
- governed reference domains that carry analytical meaning across modules

Maintafox now depends heavily on the second category.

## 2. Source Signals

### 2.1 IBM Maximo: classifications and attributes improve record finding and management

IBM Maximo documentation on categorizing work orders shows the practical value of classifications and attributes: they simplify finding and managing records by attaching controlled semantics instead of relying only on free text.

That matters for Maintafox because DI, WO, inspection, PM, and archive search quality all depend on controlled categories.

### 2.2 IBM Maximo: failure codes and failure hierarchies are governed reference structures

IBM Maximo documentation states that failure codes are linked in parent-child relationships to form failure hierarchies. It also describes failure hierarchies as structured sets of problem, cause, and remedy data used to construct accurate failure histories and support preventive action.

That is directly relevant to Maintafox because 6.10 now depends on governed failure hierarchies and 6.5 close-out quality depends on structured failure coding.

### 2.3 Maintafox already needs protected analytical reference domains

Earlier Maintafox research established several dependencies:

- 6.10 now uses versioned failure hierarchies and governed failure codes
- 6.26 explicitly protects the analytical kernel from destructive configuration changes
- 6.3 and 6.8 already depend on families, units, warehouses, and hierarchical taxonomies
- 6.24 now depends on coded cost buckets and variance drivers

This means 6.13 is not an optional convenience module. It is the shared semantic backbone behind multiple hardened modules.

## 3. Operational Purpose

The operational purpose of this module is to manage the controlled code sets, hierarchies, labels, aliases, and domain values that the rest of Maintafox depends on for:

- workflow consistency
- searchability
- reporting and KPI comparability
- reliability calculations
- inventory and purchasing traceability
- integration mapping with ERP and legacy systems

## 4. Data Capture Requirements

The module should preserve four classes of reference data.

### 4.1 Reference domain metadata

- domain code and name
- structure type such as flat list, hierarchy, versioned code set, or unit set
- governance level
- whether the domain is extendable by the tenant

### 4.2 Versioned reference values

- code, label, description, parent-child linkage
- effective version and active status
- semantic tag or analytical role where relevant
- external mapping code where relevant

### 4.3 Alias and migration support

- legacy labels and synonyms
- import aliases
- merge and replacement mappings

### 4.4 Change governance

- draft, validate, publish lifecycle
- impact report before publish
- change journal and actor

## 5. Workflow Integrity

Reference data changes need a lifecycle because semantic changes can damage historical reporting if handled casually.

Recommended minimum model:

Draft -> Validated -> Published -> Superseded

Key workflow rules:

- values already used by historical records should normally be deactivated or migrated, not deleted
- hierarchical code sets must validate parent-child rules and code uniqueness before publish
- protected analytical domains must preserve historical version meaning after publish
- merges and replacements must keep an audit trail and mapping path

## 6. Configurability Boundary

The tenant administrator should be able to configure:

- local labels, codes, colors, and hierarchy nodes for approved domains
- import and alias mappings
- sort order, visibility, and active status
- tenant-specific extensions to system-seeded domains where allowed

The tenant administrator should not be able to:

- remove the controlled semantics needed for reliability, inventory, planning, cost, and workflow analytics
- rewrite historical meanings without version tracking or migration mapping
- publish duplicate or structurally invalid hierarchies
- delete in-use reference values without dependency handling

## 7. Integration Expectations With The Rest Of Maintafox

This module must integrate tightly with:

- 6.3 Equipment Asset Registry for equipment families and asset classifications
- 6.5 Work Orders for work types, failure coding, and execution classifications
- 6.8 Inventory and Purchasing for article families, suppliers, warehouses, storage locations, units, and VAT codes
- 6.9 and 6.16 for schedule classes, priority support domains, and planning filters
- 6.10 Reliability for failure hierarchies and repeatable analytical taxonomy
- 6.24 Budget and Cost Center for spend buckets and variance-driver code sets
- 6.26 Configuration Engine for protected analytical governance, versioning, and safe publish
- 6.22 ERP Connector for external-code mapping and master-data synchronization

## 8. Bottom-Line Position For Maintafox

The design mistake would be to make 6.13 a simple list editor with import/export.

Maintafox should position this module as:

- a governed reference-domain catalog
- a versioned taxonomy and code-set manager
- a dependency-aware semantic backbone for workflows, analytics, and integrations

That is what makes the rest of the product configurable without becoming analytically incoherent.

## 9. Recommended PRD Upgrade Summary

- replace generic lookup CRUD with reference-domain governance
- add versioned domains, published sets, hierarchical validation, and impact preview
- add alias, merge, and migration support for legacy and ERP mappings
- distinguish protected analytical domains from simple local lists
- strengthen failure-code administration as a first-class governed domain
- add publish permission and deactivation-over-delete rules

## 10. Source Set

- IBM Maximo Categorizing Work Orders with Classifications and Attributes: https://www.ibm.com/docs/en/masv-and-l/maximo-manage/cd?topic=orders-categorizing-work-classifications-attributes
- IBM Maximo Failure Codes Overview: https://www.ibm.com/docs/en/masv-and-l/maximo-manage/cd?topic=codes-failure-overview
- IBM Maximo Failure Hierarchies: https://www.ibm.com/docs/en/masv-and-l/maximo-manage/cd?topic=overview-failure-hierarchies
- IBM Maximo Failure Analysis: https://www.ibm.com/docs/en/masv-and-l/maximo-manage/cd?topic=overview-failure-analysis