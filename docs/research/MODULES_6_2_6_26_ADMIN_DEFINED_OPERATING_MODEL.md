# Modules 6.2 and 6.26 Research

## Organization Model and Configuration Engine as One Admin-Defined Operating Model

### 1. Why These Two Modules Must Be Researched Together

In a serious CMMS or EAM platform, the organization model and the configuration engine cannot be designed separately.

Module 6.2 defines how the tenant's operating reality is represented:

- where assets live
- who owns maintenance responsibility
- how requests are routed
- which teams plan and execute work
- how inventory, permits, and budgets are attached
- how KPIs are aggregated

Module 6.26 defines what the administrator is allowed to change at runtime:

- states and transitions
- required fields and hidden fields
- terminology
- numbering schemes
- dashboards, queues, and layouts
- optional tenant-specific extensions

If 6.2 is rigid, then 6.26 can only provide superficial customization.

If 6.26 is too free, the tenant can damage data quality, auditability, and metric consistency.

The correct design is therefore:

- 6.2 provides the tenant-defined operating structure
- 6.26 governs how that structure and related workflows are configured safely
- both together protect the minimum evidence needed for scientific maintenance calculations

That is why these modules must be researched as one design problem.

### 2. Research Base

This brief is based on:

- current Maintafox PRD sections 6.2 and 6.26
- MaintainX documentation on locations, location settings, asset settings, and work order templates
- UpKeep documentation on request forms, internal approval forms, work order forms, custom fields, and custom work order statuses
- Fiix documentation on work request form configuration, work order form customization, and custom work order fields
- IBM Maximo documentation on organizations, sites, locations, and workflows
- ISO 14224 official summary for reliability and maintenance data philosophy
- BS EN 13306 official summary for maintenance terminology discipline

### 3. What The Sources Show

#### 3.1 MaintainX Shows Strong Field And Template Configuration

MaintainX provides a useful mid-market reference for configurable operations.

The official documentation shows that it supports:

- locations as physical places where the company operates
- sub-locations inside the location hierarchy
- custom location fields and custom asset fields
- asset statuses with configurable reasons that feed reporting
- asset hierarchy levels
- work order templates with pre-populated, hidden, required, and read-only fields

This is strong configurability, but it still operates inside a predefined product ontology centered on locations, assets, teams, work orders, and templates.

Important conclusion:

- MaintainX proves the value of configurable forms and structure-aware templates
- it does not provide a true tenant-defined operating meta-model

#### 3.2 UpKeep Shows Role- And Stage-Specific Form Governance

UpKeep's official documentation is especially useful for the configuration side of the problem.

It supports:

- request form configuration with Required, Optional, and Hidden settings
- separate field rules for creation versus approval on the internal request form
- work order form configuration with Required, Optional, and Hidden settings
- mandatory checklist tasks before work order completion
- custom work order fields and custom asset fields that feed reports and dashboards
- custom work order statuses that extend default status types such as Open, In Progress, On Hold, and Complete

Important conclusion:

- UpKeep shows that required fields must be stage-sensitive and role-sensitive
- it also shows that configurable statuses are useful only when they remain attached to stable workflow meaning

#### 3.3 Fiix Shows Scope-Aware Form Configuration

Fiix provides a useful reference for configuration scope.

Its official documentation shows:

- the work request portal can add, remove, reorder, and require fields
- Site ID and Description remain mandatory in the request form
- work order form customization can be applied at corporate, user-group, or personal scope
- work order fields and tabs can be hidden or made mandatory by user group
- custom work order fields become available across work orders once created

Important conclusion:

- Fiix shows that configuration scope matters
- it also shows that even flexible systems still preserve a small number of non-negotiable fields

#### 3.4 IBM Maximo Shows The Difference Between Power And Structural Rigidity

IBM Maximo is the strongest reference for enterprise seriousness.

The official documentation shows:

- a fixed organization -> site structure
- at least one site must exist for each organization
- locations are hierarchical functional areas used for work tracking, charging, and asset history
- workflow features automate business and record-management processes for efficiency and accountability

Important conclusion:

- Maximo is powerful, but structurally opinionated
- it demonstrates that enterprise workflow and configuration can be extensive while the core structural model remains fixed

This is useful because it reveals Maintafox's opportunity very clearly.

### 4. The Market Pattern

Across MaintainX, UpKeep, Fiix, and Maximo, the common flexibility pattern is:

1. configurable forms and fields
2. configurable statuses and workflow behavior
3. configurable visibility and layout by role or group

What is usually not configurable is the deeper structural meaning of the tenant's operating model.

Most platforms assume some fixed combination of:

- organization
- site
- location
- team
- asset

That is enough for many customers, but it is not the same as allowing the tenant to model their actual maintenance operating structure.

### 5. The Correct Maintafox Position

Maintafox should not stop at configurable labels and custom fields.

It should support a true admin-defined operating model, but with strict analytical guardrails.

The correct design has four layers.

#### Layer A: Structural Model

The tenant defines how its operating world is represented.

This includes:

- node types
- allowed parent-child rules
- ownership roles
- routing zones
- whether a node can host assets, inventory, permits, or labor planning

#### Layer B: Workflow And Form Behavior

The tenant configures:

- statuses
- transitions
- required fields by stage and role
- pre-populated or read-only fields
- numbering schemes
- queues, dashboards, and views

#### Layer C: Presentation And Terminology

The tenant configures:

- labels and module terms
- role-level visibility
- dashboard layout
- request and work order form presentation

#### Layer D: Protected Analytical Kernel

The system protects the minimum evidence required for:

- MTBF and MTTR
- backlog and response-time analytics
- failure-mode analysis
- downtime analysis
- labor and parts cost rollup
- traceability and audit

The tenant can shape the operating model, but cannot remove the core evidence fields that make the product scientifically useful.

### 6. Module 6.2 Research: Organization Model

### 6.1 Module Role In The Product

Module 6.2 should not be treated as a visual org chart feature.

It is the tenant's operating backbone.

It determines:

- where assets are installed
- which nodes can raise and own work
- who approves and plans work
- where spare parts are stocked
- how budget and responsibility roll up
- how work, failure, cost, and downtime are aggregated in analytics

If this model is weak, every other module becomes harder to configure correctly.

### 6.2 Why The Current PRD Direction Is Not Yet Strong Enough

The current PRD direction is better than a hardcoded plant hierarchy, but it still frames the structure mainly as a recursive tree of groups, plants, workshops, departments, lines, and zones.

That is better than a fixed depth limit, but it still risks three common mistakes:

1. mixing physical locations and managerial responsibility in one hierarchy
2. assuming every tenant wants the same structural semantics
3. treating organization structure as presentation only rather than as a routing and analytics control model

Scientifically useful maintenance data needs structural meaning, not just nesting.

### 6.3 The Recommended Maintafox Model

The minimum serious design is not just an unlimited tree. It is a tenant-defined structure model with typed semantics.

At minimum, Maintafox should support:

- tenant-defined node types
- allowed parent-child relationships between node types
- per-type capability flags
- named ownership roles per node
- versioned structural changes
- preserved historical associations for inactive or reorganized nodes

Recommended capability flags per node type:

- can host assets
- can host inventory
- can submit work requests
- can own work orders
- can receive permits
- can carry a cost-center code
- can appear in planning capacity views
- can aggregate KPIs

Recommended node responsibility roles:

- maintenance owner
- production owner
- HSE owner
- planner or scheduler
- approver

### 6.4 Multi-Dimension Guidance

One generic tree is often not enough in maintenance operations.

The research suggests Maintafox should be prepared for at least these structural dimensions:

- enterprise or site dimension
- physical location dimension
- responsibility or functional ownership dimension
- optional production or process dimension

Pragmatic implementation guidance:

- version 1 can start with one primary recursive node model plus typed bindings
- but the design should not assume physical location and responsibility are always the same thing

This matters because an asset can be:

- physically installed in one place
- maintained by another team
- charged to a different cost center
- planned under a different operational zone

If those relationships are collapsed carelessly, later analytics become misleading.

### 6.5 Recommended Data Model Direction

The research direction for 6.2 is closer to this model:

- `org_structure_models`: id, name, version, effective_from, effective_to, status
- `org_node_types`: id, model_id, code, label, icon, color_hex, capability_flags_json, is_system, is_active
- `org_type_relationship_rules`: id, model_id, parent_type_id, child_type_id, min_occurs, max_occurs, is_allowed
- `org_nodes`: id, model_id, type_id, parent_id, code, name, description, is_active, opened_at, closed_at
- `org_node_responsibilities`: id, node_id, role_code, personnel_id, group_id, valid_from, valid_to
- `org_entity_bindings`: id, entity_type, entity_id, node_id, binding_role
- `companies`: id, legal_name, display_name, address, currency, logo_path, license_key, created_at

The important architectural idea is not the exact table names. The important point is that node semantics, allowed relationships, and ownership are configurable data, not hardcoded assumptions.

### 6.6 UX Requirements

The admin experience should include:

- structure designer with drag-and-drop or guided creation
- allowed-relationship validation before save
- impact preview before moving or disabling a node
- bulk import and mapping tools
- node capability editor
- responsibility assignment panel
- search and dependency explorer showing linked assets, people, open work, inventory, and permits

The system should prevent silent structural damage.

Before a major structural change, the admin should see the operational impact.

### 6.7 Configurability Rules

The administrator should be able to configure:

- node types and labels
- allowed nesting rules
- node capabilities
- default owner-role templates by node type
- naming and code conventions
- which dimensions are enabled for the tenant

The administrator should not be able to:

- delete nodes with historical work evidence without archival handling
- retroactively erase structural context from closed records
- remove all valid work-owning nodes from a live tenant
- publish invalid parent-child models that strand assets or people

### 6.8 Corrections Recommended For The Current PRD 6.2

1. Change the objective from a fixed company hierarchy toward a tenant-defined operating structure.
2. Replace the implied fixed type list with admin-defined node types and relationship rules.
3. Add node capability flags and responsibility-role bindings.
4. Explicitly separate physical location semantics from responsibility semantics, or support typed bindings that preserve the difference.
5. Add versioning, effective dating, and impact-preview requirements.
6. Protect historical reporting when nodes are renamed, merged, or deactivated.

### 7. Module 6.26 Research: Configuration Engine

### 7.1 Module Role In The Product

Module 6.26 is not just a settings screen.

It is the governance layer that decides which parts of the tenant's operating model are configurable at runtime, how those changes are validated, and how historical consistency is preserved.

Its job is to make the tenant flexible without making the product unreliable.

### 7.2 Research-Backed Configuration Principles

The sources support several principles very clearly.

#### Principle 1: Configurable Does Not Mean Structureless

UpKeep, Fiix, and MaintainX all support configuration, but inside defined boundaries.

Maintafox should do the same.

#### Principle 2: Required Fields Must Be Stage-Specific

UpKeep's create-versus-approval controls are a strong reference.

Maintafox should allow field requirements to differ by:

- role
- workflow state
- transaction stage
- record type

#### Principle 3: Templates Need More Than Defaults

MaintainX shows the value of hidden, required, read-only, and pre-populated fields.

Maintafox should support the same behavior for DI, WO, inspection, permit, and asset-related forms.

#### Principle 4: Workflow Configuration Needs Semantic Guardrails

UpKeep custom statuses and Maximo workflows show that configurable process control is valuable.

But a completely free-form state model is dangerous because it can break:

- analytics
- cross-module integration
- audit traceability
- reporting consistency

#### Principle 5: Historical Records Must Keep Their Original Meaning

When a configuration changes, historical records must retain the configuration version they were created under, or at least keep traceable mappings.

Without this, trend analysis becomes unreliable.

### 7.3 Workflow Research Conclusions For Maintafox

The current PRD state-machine designer is directionally strong, but it should be constrained more carefully.

Recommended rule:

- tenants can rename states, add allowed intermediate states, and configure transitions
- but every workflow still maps to protected semantic macro-states

Recommended semantic macro-states for serious maintenance flows:

- requested
- under_review
- approved
- scheduled
- waiting
- executing
- completed
- verified
- closed
- cancelled
- archived

This means the tenant can express site-specific language without breaking the product's understanding of what those states mean analytically.

### 7.4 Field And Form Governance Requirements

The configuration engine should support:

- required, optional, hidden, and read-only rules
- different field rules by role and workflow stage
- pre-populated values from templates
- controlled custom field types
- list, filter, export, and reporting behavior per field
- field visibility by permission
- corporate, role, team, and user-level layout scope where appropriate

It should also support a distinction between:

- analytics-enabled fields that can be used in filters, KPIs, and exports
- narrative-only fields that are stored for context but do not drive calculations

### 7.5 Configuration Governance Requirements

The research strongly supports adding the following controls to 6.26:

- versioning for workflows, forms, and structural rules
- preview or sandbox mode before publish
- impact analysis before publish
- audit log of configuration changes
- configuration import and export
- deactivation instead of destructive deletion for states and fields already in historical use
- migration tools when statuses, priorities, or required fields are changed

### 7.6 The Protected Analytical Kernel

This is the most important conclusion for Maintafox.

The tenant should not be allowed to configure away the fields required for scientific maintenance calculations.

Examples of protected fields and concepts include:

- record identifier and timestamps
- asset or location context
- origin or work type
- requester and responsible owner
- workflow state history
- validated priority or urgency
- source request to work order linkage
- labor actuals
- parts actuals or explicit none-used marker
- delay segments and delay reasons
- downtime segments
- failure coding or cause-not-determined marker for corrective work
- closure verification and return-to-service confirmation

The exact mandatory set can vary by module and work type, but the platform must protect the minimum structure needed for calculation, traceability, and audit.

### 7.7 Corrections Recommended For The Current PRD 6.26

1. Add semantic macro-state guardrails to the workflow designer.
2. Add stage-specific and role-specific field requirement rules.
3. Add hidden, read-only, and pre-populated template behavior as a first-class configuration capability.
4. Add preview, publish, rollback, and impact-analysis workflow for configuration changes.
5. Add version tracking so historical records remain analyzable after configuration changes.
6. Explicitly distinguish protected analytical fields from optional tenant customizations.
7. Prefer deactivation and migration over destructive deletion for states, fields, and levels in live use.

### 8. Bottom-Line Position For Maintafox

Most competitor systems let administrators customize forms, fields, labels, and some statuses.

Maintafox should go one level deeper.

It should let the administrator define the tenant's operating model itself, while still enforcing the evidence discipline required for real maintenance analytics.

That means:

- structure is tenant-defined
- workflows are configurable within semantic guardrails
- forms are role- and stage-aware
- history is versioned and auditable
- analytical core evidence cannot be disabled away

That is the correct professional position if Maintafox wants to be both adaptable and scientifically credible.

### 9. Source Set

- MaintainX About Locations: https://help.getmaintainx.com/about-locations
- MaintainX Location Settings: https://help.getmaintainx.com/location-settings
- MaintainX Asset Settings: https://help.getmaintainx.com/asset-settings
- MaintainX Create a Work Order Template: https://help.getmaintainx.com/create-a-work-order-template
- UpKeep Manage Request Forms: https://help.onupkeep.com/en/articles/5617700-overview-manage-your-request-forms
- UpKeep Configure the Internal Request Form: https://help.onupkeep.com/en/articles/5617701-configure-the-internal-request-form
- UpKeep Manage Work Order Forms: https://help.onupkeep.com/en/articles/7677855-how-to-edit-manage-work-order-forms
- UpKeep Work Order Custom Fields: https://help.onupkeep.com/en/articles/10984737-how-to-use-work-order-custom-fields
- UpKeep Asset Custom Fields: https://help.onupkeep.com/en/articles/10344979-how-to-create-edit-and-remove-custom-asset-fields
- UpKeep Custom Work Order Statuses: https://help.onupkeep.com/en/articles/9145531-how-to-create-custom-work-order-statuses
- Fiix Configure the Work Request Form: https://helpdesk.fiixsoftware.com/hc/en-us/articles/14526174236308-Configure-the-work-request-form
- Fiix Customize the Work Order Form: https://helpdesk.fiixsoftware.com/hc/en-us/articles/1500006492721-Customize-the-work-order-form
- Fiix Create Custom Fields for Work Orders: https://helpdesk.fiixsoftware.com/hc/en-us/articles/360046996972-Create-custom-fields-for-work-orders
- IBM Maximo Sites and Organizations Overview: https://www.ibm.com/docs/en/masv-and-l/maximo-manage/cd?topic=overview-sites-organizations
- IBM Maximo Locations Overview: https://www.ibm.com/docs/en/masv-and-l/maximo-manage/cd?topic=locations-overview
- IBM Maximo Workflows: https://www.ibm.com/docs/en/masv-and-l/maximo-manage/cd?topic=administering-workflows
- ISO 14224:2016 official summary: https://www.iso.org/standard/64076.html
- BS EN 13306:2017 official summary: https://knowledge.bsigroup.com/products/maintenance-maintenance-terminology