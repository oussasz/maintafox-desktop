use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait, Value};

use crate::errors::{AppError, AppResult};
use crate::inventory::domain::{
    ArticleFamily, CreateArticleFamilyInput, CreateStockLocationInput, CreateWarehouseInput, InventoryArticle,
    InventoryArticleFilter, InventoryArticleInput, InventoryIssueInput, InventoryReleaseReservationInput,
    InventoryReorderRecommendation, InventoryReserveInput, InventoryReturnInput, InventoryStockAdjustInput,
    InventoryStockBalance, InventoryStockFilter, InventoryTaxCategory, InventoryTaxCategoryInput, InventoryTransaction,
    InventoryTransactionFilter, InventoryTransferInput, StockImpactProjection, StockLocation, StockReservation,
    StockReservationFilter, UpdateArticleFamilyInput, UpdateStockLocationInput, UpdateWarehouseInput, Warehouse,
};
use crate::inventory::procurement::record_state_event;

fn parse_bool_to_i64(value: Option<bool>, default_true: bool) -> i64 {
    match value {
        Some(true) => 1,
        Some(false) => 0,
        None => i64::from(default_true),
    }
}

async fn get_lookup_domain_id(db: &DatabaseConnection, domain_key: &str) -> AppResult<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM lookup_domains WHERE domain_key = ? AND deleted_at IS NULL",
            [domain_key.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Err(AppError::ValidationFailed(vec![format!(
            "Lookup domain '{domain_key}' does not exist."
        )]));
    };
    Ok(row.try_get("", "id")?)
}

fn validate_article_stock_contract(input: &InventoryArticleInput) -> AppResult<()> {
    if input.min_stock < 0.0 || input.reorder_point < 0.0 || input.safety_stock < 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "Stock thresholds cannot be negative.".to_string()
        ]));
    }
    if let Some(max_stock) = input.max_stock {
        if max_stock < 0.0 {
            return Err(AppError::ValidationFailed(vec![
                "max_stock cannot be negative.".to_string()
            ]));
        }
        if max_stock < input.min_stock {
            return Err(AppError::ValidationFailed(vec![
                "max_stock must be greater than or equal to min_stock.".to_string(),
            ]));
        }
        if max_stock < input.reorder_point {
            return Err(AppError::ValidationFailed(vec![
                "max_stock must be greater than or equal to reorder_point.".to_string(),
            ]));
        }
    }
    if input.reorder_point < input.min_stock {
        return Err(AppError::ValidationFailed(vec![
            "reorder_point must be greater than or equal to min_stock.".to_string(),
        ]));
    }
    Ok(())
}

async fn ensure_lookup_value_in_domain(
    db: &DatabaseConnection,
    value_id: i64,
    expected_domain_key: &str,
    field_name: &str,
) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r#"
            SELECT ld.domain_key
            FROM lookup_values lv
            JOIN lookup_domains ld ON ld.id = lv.domain_id
            WHERE lv.id = ? AND lv.deleted_at IS NULL
            "#,
            [value_id.into()],
        ))
        .await?;

    let Some(row) = row else {
        return Err(AppError::ValidationFailed(vec![format!(
            "{field_name} does not exist."
        )]));
    };

    let domain_key: String = row.try_get("", "domain_key")?;
    if domain_key != expected_domain_key {
        return Err(AppError::ValidationFailed(vec![format!(
            "{field_name} must reference domain '{expected_domain_key}'."
        )]));
    }

    Ok(())
}

async fn ensure_article_family_active(db: &DatabaseConnection, family_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT is_active FROM article_families WHERE id = ?",
            [family_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Err(AppError::ValidationFailed(
            vec!["family_id does not exist.".to_string()],
        ));
    };
    let is_active: i64 = row.try_get("", "is_active")?;
    if is_active == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Cannot assign inactive family to article.".to_string(),
        ]));
    }
    Ok(())
}

async fn ensure_preferred_location_hint(
    db: &DatabaseConnection,
    preferred_warehouse_id: Option<i64>,
    preferred_location_id: Option<i64>,
) -> AppResult<()> {
    if preferred_warehouse_id.is_none() && preferred_location_id.is_some() {
        return Err(AppError::ValidationFailed(vec![
            "preferred_warehouse_id is required when preferred_location_id is provided.".to_string(),
        ]));
    }

    if let Some(warehouse_id) = preferred_warehouse_id {
        let warehouse_row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT is_active FROM warehouses WHERE id = ?",
                [warehouse_id.into()],
            ))
            .await?;
        let Some(warehouse_row) = warehouse_row else {
            return Err(AppError::ValidationFailed(vec![
                "preferred_warehouse_id does not exist.".to_string(),
            ]));
        };
        let warehouse_active: i64 = warehouse_row.try_get("", "is_active")?;
        if warehouse_active == 0 {
            return Err(AppError::ValidationFailed(vec![
                "preferred_warehouse_id must reference an active warehouse.".to_string(),
            ]));
        }
    }

    if let Some(location_id) = preferred_location_id {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT warehouse_id, is_active FROM stock_locations WHERE id = ?",
                [location_id.into()],
            ))
            .await?;
        let Some(row) = row else {
            return Err(AppError::ValidationFailed(vec![
                "preferred_location_id does not exist.".to_string(),
            ]));
        };
        let location_wh: i64 = row.try_get("", "warehouse_id")?;
        let location_active: i64 = row.try_get("", "is_active")?;
        if location_active == 0 {
            return Err(AppError::ValidationFailed(vec![
                "preferred_location_id must reference an active location.".to_string(),
            ]));
        }
        if Some(location_wh) != preferred_warehouse_id {
            return Err(AppError::ValidationFailed(vec![
                "preferred_location_id must belong to preferred_warehouse_id.".to_string(),
            ]));
        }
    }
    Ok(())
}

pub async fn list_article_families(db: &DatabaseConnection) -> AppResult<Vec<ArticleFamily>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, code, name, description, is_active, created_at, updated_at
             FROM article_families
             ORDER BY code ASC"
                .to_string(),
        ))
        .await?;

    rows.into_iter()
        .map(|row| {
            Ok(ArticleFamily {
                id: row.try_get("", "id")?,
                code: row.try_get("", "code")?,
                name: row.try_get("", "name")?,
                description: row.try_get("", "description")?,
                is_active: row.try_get("", "is_active")?,
                created_at: row.try_get("", "created_at")?,
                updated_at: row.try_get("", "updated_at")?,
            })
        })
        .collect()
}

pub async fn create_article_family(
    db: &DatabaseConnection,
    input: CreateArticleFamilyInput,
) -> AppResult<ArticleFamily> {
    let code = input.code.trim().to_uppercase();
    let name = input.name.trim().to_string();
    if code.is_empty() || name.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Family code and name are required.".to_string()
        ]));
    }

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO article_families (code, name, description, is_active)
         VALUES (?, ?, ?, 1)",
        [code.clone().into(), name.clone().into(), input.description.into()],
    ))
    .await?;

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, code, name, description, is_active, created_at, updated_at
             FROM article_families
             WHERE code = ?",
            [code.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("created family not found")))?;

    Ok(ArticleFamily {
        id: row.try_get("", "id")?,
        code: row.try_get("", "code")?,
        name: row.try_get("", "name")?,
        description: row.try_get("", "description")?,
        is_active: row.try_get("", "is_active")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

pub async fn update_article_family(
    db: &DatabaseConnection,
    family_id: i64,
    input: UpdateArticleFamilyInput,
) -> AppResult<ArticleFamily> {
    let code = input.code.trim().to_uppercase();
    let name = input.name.trim().to_string();
    if code.is_empty() || name.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Family code and name are required.".to_string()
        ]));
    }
    let is_active = parse_bool_to_i64(input.is_active, true);

    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE article_families
             SET code = ?, name = ?, description = ?, is_active = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE id = ?",
            [
                code.into(),
                name.into(),
                input.description.into(),
                is_active.into(),
                family_id.into(),
            ],
        ))
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound {
            entity: "ArticleFamily".to_string(),
            id: family_id.to_string(),
        });
    }

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, code, name, description, is_active, created_at, updated_at
             FROM article_families
             WHERE id = ?",
            [family_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("updated family not found")))?;

    Ok(ArticleFamily {
        id: row.try_get("", "id")?,
        code: row.try_get("", "code")?,
        name: row.try_get("", "name")?,
        description: row.try_get("", "description")?,
        is_active: row.try_get("", "is_active")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

pub async fn deactivate_article_family(db: &DatabaseConnection, family_id: i64) -> AppResult<ArticleFamily> {
    let current_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT code, name, description FROM article_families WHERE id = ?",
            [family_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "ArticleFamily".to_string(),
            id: family_id.to_string(),
        })?;

    let code: String = current_row.try_get("", "code")?;
    let name: String = current_row.try_get("", "name")?;
    let description: Option<String> = current_row.try_get("", "description")?;

    let usage_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM articles WHERE family_id = ? AND is_active = 1",
            [family_id.into()],
        ))
        .await?;
    let usage_count: i64 = usage_row
        .as_ref()
        .and_then(|row| row.try_get("", "cnt").ok())
        .unwrap_or(0);
    if usage_count > 0 {
        return Err(AppError::ValidationFailed(vec![
            "Cannot deactivate family because active articles still reference it.".to_string(),
        ]));
    }

    update_article_family(
        db,
        family_id,
        UpdateArticleFamilyInput {
            code,
            name,
            description,
            is_active: Some(false),
        },
    )
    .await
}

pub async fn list_inventory_tax_categories(db: &DatabaseConnection) -> AppResult<Vec<InventoryTaxCategory>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT lv.id, lv.code, lv.label, lv.fr_label, lv.en_label, lv.description,
                    lv.sort_order, lv.is_active, lv.row_version, lv.created_at, lv.updated_at
             FROM lookup_values lv
             JOIN lookup_domains ld ON ld.id = lv.domain_id
             WHERE ld.domain_key = 'inventory.tax_category' AND lv.deleted_at IS NULL
             ORDER BY lv.sort_order ASC, lv.code ASC",
            [],
        ))
        .await?;

    rows.into_iter()
        .map(|row| {
            Ok(InventoryTaxCategory {
                id: row.try_get("", "id")?,
                code: row.try_get("", "code")?,
                label: row.try_get("", "label")?,
                fr_label: row.try_get("", "fr_label")?,
                en_label: row.try_get("", "en_label")?,
                description: row.try_get("", "description")?,
                sort_order: row.try_get("", "sort_order")?,
                is_active: row.try_get("", "is_active")?,
                row_version: row.try_get("", "row_version")?,
                created_at: row.try_get("", "created_at")?,
                updated_at: row.try_get("", "updated_at")?,
            })
        })
        .collect()
}

pub async fn create_inventory_tax_category(
    db: &DatabaseConnection,
    input: InventoryTaxCategoryInput,
) -> AppResult<InventoryTaxCategory> {
    let code = input.code.trim().to_uppercase();
    let label = input.label.trim().to_string();
    if code.is_empty() || label.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Tax category code and label are required.".to_string(),
        ]));
    }

    let domain_id = get_lookup_domain_id(db, "inventory.tax_category").await?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(MAX(sort_order), 0) + 1 AS next_sort
             FROM lookup_values WHERE domain_id = ? AND deleted_at IS NULL",
            [domain_id.into()],
        ))
        .await?;
    let next_sort_order: i64 = row.as_ref().and_then(|r| r.try_get("", "next_sort").ok()).unwrap_or(1);

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO lookup_values (
            entity_sync_id, domain_id, code, label, fr_label, en_label, description,
            sort_order, is_active, is_system, color, parent_value_id, metadata_json,
            created_at, updated_at, row_version
         )
         VALUES (lower(hex(randomblob(16))), ?, ?, ?, ?, ?, ?, ?, 1, 0, NULL, NULL, NULL,
                 strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1)",
        [
            domain_id.into(),
            code.clone().into(),
            label.into(),
            input.fr_label.into(),
            input.en_label.into(),
            input.description.into(),
            next_sort_order.into(),
        ],
    ))
    .await
    .map_err(|e| {
        let message = e.to_string();
        if message.contains("UNIQUE constraint failed") {
            AppError::ValidationFailed(vec!["Tax category code already exists.".to_string()])
        } else {
            AppError::from(e)
        }
    })?;

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT lv.id, lv.code, lv.label, lv.fr_label, lv.en_label, lv.description,
                    lv.sort_order, lv.is_active, lv.row_version, lv.created_at, lv.updated_at
             FROM lookup_values lv
             JOIN lookup_domains ld ON ld.id = lv.domain_id
             WHERE ld.domain_key = 'inventory.tax_category' AND lv.code = ? AND lv.deleted_at IS NULL",
            [code.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("created tax category not found")))?;

    Ok(InventoryTaxCategory {
        id: row.try_get("", "id")?,
        code: row.try_get("", "code")?,
        label: row.try_get("", "label")?,
        fr_label: row.try_get("", "fr_label")?,
        en_label: row.try_get("", "en_label")?,
        description: row.try_get("", "description")?,
        sort_order: row.try_get("", "sort_order")?,
        is_active: row.try_get("", "is_active")?,
        row_version: row.try_get("", "row_version")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

pub async fn update_inventory_tax_category(
    db: &DatabaseConnection,
    tax_category_id: i64,
    expected_row_version: i64,
    input: InventoryTaxCategoryInput,
) -> AppResult<InventoryTaxCategory> {
    let code = input.code.trim().to_uppercase();
    let label = input.label.trim().to_string();
    if code.is_empty() || label.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Tax category code and label are required.".to_string(),
        ]));
    }

    let domain_id = get_lookup_domain_id(db, "inventory.tax_category").await?;
    let conflicting = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM lookup_values
             WHERE domain_id = ? AND code = ? AND id <> ? AND deleted_at IS NULL",
            [domain_id.into(), code.clone().into(), tax_category_id.into()],
        ))
        .await?;
    if conflicting.is_some() {
        return Err(AppError::ValidationFailed(vec![
            "Tax category code already exists.".to_string()
        ]));
    }

    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE lookup_values
             SET code = ?, label = ?, fr_label = ?, en_label = ?, description = ?,
                 row_version = row_version + 1,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE id = ? AND domain_id = ? AND deleted_at IS NULL AND row_version = ?",
            [
                code.into(),
                label.into(),
                input.fr_label.into(),
                input.en_label.into(),
                input.description.into(),
                tax_category_id.into(),
                domain_id.into(),
                expected_row_version.into(),
            ],
        ))
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Tax category update failed (not found or stale row_version).".to_string(),
        ]));
    }

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT lv.id, lv.code, lv.label, lv.fr_label, lv.en_label, lv.description,
                    lv.sort_order, lv.is_active, lv.row_version, lv.created_at, lv.updated_at
             FROM lookup_values lv
             JOIN lookup_domains ld ON ld.id = lv.domain_id
             WHERE ld.domain_key = 'inventory.tax_category' AND lv.id = ? AND lv.deleted_at IS NULL",
            [tax_category_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("updated tax category not found")))?;

    Ok(InventoryTaxCategory {
        id: row.try_get("", "id")?,
        code: row.try_get("", "code")?,
        label: row.try_get("", "label")?,
        fr_label: row.try_get("", "fr_label")?,
        en_label: row.try_get("", "en_label")?,
        description: row.try_get("", "description")?,
        sort_order: row.try_get("", "sort_order")?,
        is_active: row.try_get("", "is_active")?,
        row_version: row.try_get("", "row_version")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

pub async fn deactivate_inventory_tax_category(
    db: &DatabaseConnection,
    tax_category_id: i64,
    expected_row_version: i64,
) -> AppResult<InventoryTaxCategory> {
    let usage_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COUNT(*) AS cnt FROM articles WHERE tax_category_value_id = ? AND is_active = 1",
            [tax_category_id.into()],
        ))
        .await?;
    let usage_count: i64 = usage_row
        .as_ref()
        .and_then(|row| row.try_get("", "cnt").ok())
        .unwrap_or(0);
    if usage_count > 0 {
        return Err(AppError::ValidationFailed(vec![
            "Cannot deactivate tax category because active articles still reference it.".to_string(),
        ]));
    }

    let domain_id = get_lookup_domain_id(db, "inventory.tax_category").await?;
    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE lookup_values
             SET is_active = 0,
                 row_version = row_version + 1,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE id = ? AND domain_id = ? AND deleted_at IS NULL AND row_version = ?",
            [tax_category_id.into(), domain_id.into(), expected_row_version.into()],
        ))
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Tax category deactivation failed (not found or stale row_version).".to_string(),
        ]));
    }

    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT lv.id, lv.code, lv.label, lv.fr_label, lv.en_label, lv.description,
                    lv.sort_order, lv.is_active, lv.row_version, lv.created_at, lv.updated_at
             FROM lookup_values lv
             JOIN lookup_domains ld ON ld.id = lv.domain_id
             WHERE ld.domain_key = 'inventory.tax_category' AND lv.id = ? AND lv.deleted_at IS NULL",
            [tax_category_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("deactivated tax category not found")))?;

    Ok(InventoryTaxCategory {
        id: row.try_get("", "id")?,
        code: row.try_get("", "code")?,
        label: row.try_get("", "label")?,
        fr_label: row.try_get("", "fr_label")?,
        en_label: row.try_get("", "en_label")?,
        description: row.try_get("", "description")?,
        sort_order: row.try_get("", "sort_order")?,
        is_active: row.try_get("", "is_active")?,
        row_version: row.try_get("", "row_version")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
    })
}

pub async fn list_warehouses(db: &DatabaseConnection) -> AppResult<Vec<Warehouse>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, code, name, is_active, created_at
             FROM warehouses
             ORDER BY code ASC"
                .to_string(),
        ))
        .await?;

    rows.into_iter()
        .map(|row| {
            Ok(Warehouse {
                id: row.try_get("", "id")?,
                code: row.try_get("", "code")?,
                name: row.try_get("", "name")?,
                is_active: row.try_get("", "is_active")?,
                created_at: row.try_get("", "created_at")?,
            })
        })
        .collect()
}

pub async fn list_locations(db: &DatabaseConnection, warehouse_id: Option<i64>) -> AppResult<Vec<StockLocation>> {
    let rows = if let Some(warehouse_id) = warehouse_id {
        db.query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT sl.id, sl.warehouse_id, w.code AS warehouse_code, sl.code, sl.name, sl.is_default, sl.is_active,
                    sl.created_at, sl.updated_at, sl.row_version
             FROM stock_locations sl
             JOIN warehouses w ON w.id = sl.warehouse_id
             WHERE sl.warehouse_id = ?
             ORDER BY w.code, sl.code",
            [warehouse_id.into()],
        ))
        .await?
    } else {
        db.query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT sl.id, sl.warehouse_id, w.code AS warehouse_code, sl.code, sl.name, sl.is_default, sl.is_active,
                    sl.created_at, sl.updated_at, sl.row_version
             FROM stock_locations sl
             JOIN warehouses w ON w.id = sl.warehouse_id
             ORDER BY w.code, sl.code"
                .to_string(),
        ))
        .await?
    };

    rows.into_iter()
        .map(|row| {
            Ok(StockLocation {
                id: row.try_get("", "id")?,
                warehouse_id: row.try_get("", "warehouse_id")?,
                warehouse_code: row.try_get("", "warehouse_code")?,
                code: row.try_get("", "code")?,
                name: row.try_get("", "name")?,
                is_default: row.try_get("", "is_default")?,
                is_active: row.try_get("", "is_active")?,
                created_at: row.try_get("", "created_at")?,
                updated_at: row.try_get("", "updated_at")?,
                row_version: row.try_get("", "row_version")?,
            })
        })
        .collect()
}

pub async fn create_warehouse(db: &DatabaseConnection, input: CreateWarehouseInput) -> AppResult<Warehouse> {
    let code = input.code.trim().to_string();
    let name = input.name.trim().to_string();
    if code.is_empty() || name.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Warehouse code and name are required.".to_string(),
        ]));
    }
    let tx = db.begin().await?;
    let dup = tx
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM warehouses WHERE code = ? COLLATE NOCASE",
            [code.clone().into()],
        ))
        .await?;
    if dup.is_some() {
        return Err(AppError::ValidationFailed(vec![format!(
            "Warehouse code '{code}' already exists."
        )]));
    }
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO warehouses (entity_sync_id, code, name, is_active, row_version) VALUES (?, ?, ?, 1, 1)",
        [
            crate::sync::identity::new_entity_sync_id().into(),
            code.into(),
            name.into(),
        ],
    ))
    .await?;
    let row = tx
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, code, name, is_active, created_at FROM warehouses WHERE id = last_insert_rowid()".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("warehouse insert")))?;
    let warehouse = Warehouse {
        id: row.try_get("", "id")?,
        code: row.try_get("", "code")?,
        name: row.try_get("", "name")?,
        is_active: row.try_get("", "is_active")?,
        created_at: row.try_get("", "created_at")?,
    };
    tx.commit().await?;
    Ok(warehouse)
}

pub async fn update_warehouse(
    db: &DatabaseConnection,
    warehouse_id: i64,
    input: UpdateWarehouseInput,
) -> AppResult<Warehouse> {
    let mut sets: Vec<&'static str> = Vec::new();
    let mut vals: Vec<sea_orm::Value> = Vec::new();
    if let Some(ref name) = input.name {
        let n = name.trim();
        if n.is_empty() {
            return Err(AppError::ValidationFailed(vec![
                "Warehouse name cannot be empty.".to_string()
            ]));
        }
        sets.push("name = ?");
        vals.push(n.to_string().into());
    }
    if let Some(active) = input.is_active {
        sets.push("is_active = ?");
        vals.push(i64::from(active).into());
    }
    if sets.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "No warehouse fields to update.".to_string()
        ]));
    }
    vals.push(warehouse_id.into());
    let sql = format!("UPDATE warehouses SET {} WHERE id = ?", sets.join(", "));
    let tx = db.begin().await?;
    let result = tx
        .execute(Statement::from_sql_and_values(DbBackend::Sqlite, sql, vals))
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound {
            entity: "Warehouse".to_string(),
            id: warehouse_id.to_string(),
        });
    }
    let row = tx
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, code, name, is_active, created_at FROM warehouses WHERE id = ?",
            [warehouse_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "Warehouse".to_string(),
            id: warehouse_id.to_string(),
        })?;
    let warehouse = Warehouse {
        id: row.try_get("", "id")?,
        code: row.try_get("", "code")?,
        name: row.try_get("", "name")?,
        is_active: row.try_get("", "is_active")?,
        created_at: row.try_get("", "created_at")?,
    };
    tx.commit().await?;
    Ok(warehouse)
}

pub async fn create_stock_location(
    db: &DatabaseConnection,
    input: CreateStockLocationInput,
) -> AppResult<StockLocation> {
    let code = input.code.trim().to_string();
    let name = input.name.trim().to_string();
    if code.is_empty() || name.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Location code and name are required.".to_string()
        ]));
    }
    let wh = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, is_active FROM warehouses WHERE id = ?",
            [input.warehouse_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "Warehouse".to_string(),
            id: input.warehouse_id.to_string(),
        })?;
    let wh_active: i64 = wh.try_get("", "is_active")?;
    if wh_active == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Cannot add a location to an inactive warehouse.".to_string(),
        ]));
    }
    let dup = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM stock_locations WHERE warehouse_id = ? AND code = ? COLLATE NOCASE",
            [input.warehouse_id.into(), code.clone().into()],
        ))
        .await?;
    if dup.is_some() {
        return Err(AppError::ValidationFailed(vec![format!(
            "Location code '{code}' already exists in this warehouse."
        )]));
    }

    let tx = db.begin().await?;
    let make_default = input.is_default.unwrap_or(false);
    if make_default {
        tx.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE stock_locations SET is_default = 0 WHERE warehouse_id = ?",
            [input.warehouse_id.into()],
        ))
        .await?;
    }
    let entity_sync_id = crate::sync::identity::new_entity_sync_id();
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO stock_locations (entity_sync_id, warehouse_id, code, name, is_default, is_active, row_version)
         VALUES (?, ?, ?, ?, ?, 1, 1)",
        [
            entity_sync_id.into(),
            input.warehouse_id.into(),
            code.into(),
            name.into(),
            i64::from(make_default).into(),
        ],
    ))
    .await?;
    let id_row = tx
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("location insert")))?;
    let new_id: i64 = id_row.try_get("", "id")?;
    tx.commit().await?;

    list_locations(db, Some(input.warehouse_id))
        .await?
        .into_iter()
        .find(|l| l.id == new_id)
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("created location not found")))
}

pub async fn update_stock_location(
    db: &DatabaseConnection,
    location_id: i64,
    expected_row_version: i64,
    input: UpdateStockLocationInput,
) -> AppResult<StockLocation> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, warehouse_id, code, name, is_default, is_active, row_version
             FROM stock_locations WHERE id = ?",
            [location_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "stock_locations".to_string(),
            id: location_id.to_string(),
        })?;
    let warehouse_id: i64 = row.try_get("", "warehouse_id")?;
    let current_rv: i64 = row.try_get("", "row_version")?;
    if current_rv != expected_row_version {
        return Err(AppError::ValidationFailed(vec![
            "Location was modified elsewhere (stale row_version).".to_string(),
        ]));
    }

    let mut sets: Vec<String> = Vec::new();
    let mut vals: Vec<sea_orm::Value> = Vec::new();
    if let Some(ref c) = input.code {
        let code = c.trim().to_string();
        if code.is_empty() {
            return Err(AppError::ValidationFailed(vec![
                "Location code cannot be empty.".to_string()
            ]));
        }
        let dup = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT id FROM stock_locations WHERE warehouse_id = ? AND code = ? COLLATE NOCASE AND id != ?",
                [warehouse_id.into(), code.clone().into(), location_id.into()],
            ))
            .await?;
        if dup.is_some() {
            return Err(AppError::ValidationFailed(vec![format!(
                "Location code '{code}' already exists in this warehouse."
            )]));
        }
        sets.push("code = ?".to_string());
        vals.push(code.into());
    }
    if let Some(ref n) = input.name {
        let name = n.trim().to_string();
        if name.is_empty() {
            return Err(AppError::ValidationFailed(vec![
                "Location name cannot be empty.".to_string()
            ]));
        }
        sets.push("name = ?".to_string());
        vals.push(name.into());
    }
    if let Some(d) = input.is_default {
        sets.push("is_default = ?".to_string());
        vals.push(i64::from(d).into());
    }
    if let Some(a) = input.is_active {
        sets.push("is_active = ?".to_string());
        vals.push(i64::from(a).into());
    }
    if sets.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "No location fields to update.".to_string()
        ]));
    }
    sets.push("row_version = row_version + 1".to_string());
    sets.push("updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')".to_string());

    let tx = db.begin().await?;
    if let Some(true) = input.is_default {
        tx.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE stock_locations SET is_default = 0 WHERE warehouse_id = ? AND id != ?",
            [warehouse_id.into(), location_id.into()],
        ))
        .await?;
    }
    let sql = format!(
        "UPDATE stock_locations SET {} WHERE id = ? AND row_version = ?",
        sets.join(", ")
    );
    vals.push(location_id.into());
    vals.push(expected_row_version.into());
    let result = tx
        .execute(Statement::from_sql_and_values(DbBackend::Sqlite, sql, vals))
        .await?;
    if result.rows_affected() == 0 {
        tx.rollback().await?;
        return Err(AppError::ValidationFailed(vec![
            "Location update failed (not found or stale row_version).".to_string(),
        ]));
    }
    tx.commit().await?;

    list_locations(db, Some(warehouse_id))
        .await?
        .into_iter()
        .find(|l| l.id == location_id)
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("updated location not found")))
}

/// Article master rows with their stock totals summed over every location, in one query
/// (no per-article balance calls from the UI).
const ARTICLE_LIST_SELECT_TEMPLATE: &str = r#"
    SELECT a.id, a.article_code, a.article_name, a.family_id, af.code AS family_code, af.name AS family_name,
           a.unit_value_id, u.code AS unit_code, u.label AS unit_label, {UNIT_SYMBOL} AS unit_symbol,
           a.criticality_value_id, c.code AS criticality_code, c.label AS criticality_label,
           COALESCE(a.stocking_type_value_id, 0) AS stocking_type_value_id,
           COALESCE(st.code, '') AS stocking_type_code,
           COALESCE(st.label, '') AS stocking_type_label,
           COALESCE(a.tax_category_value_id, 0) AS tax_category_value_id,
           COALESCE(tx.code, '') AS tax_category_code,
           COALESCE(tx.label, '') AS tax_category_label,
           a.procurement_category_value_id, pc.code AS procurement_category_code, pc.label AS procurement_category_label,
           a.preferred_warehouse_id, pw.code AS preferred_warehouse_code, pw.name AS preferred_warehouse_name,
           a.preferred_location_id, pl.code AS preferred_location_code, pl.name AS preferred_location_name,
           a.min_stock, a.max_stock, a.reorder_point, a.safety_stock,
           a.manufacturer_name, a.manufacturer_part_number, a.oem_part_number,
           a.replenishment_policy_code, a.economic_order_qty, a.minimum_order_qty, a.maximum_order_qty,
           a.order_multiple_qty, a.lead_time_days, a.review_period_days,
           a.abc_class_code, a.xyz_class_code,
           COALESCE(a.is_critical_spare, 0) AS is_critical_spare,
           COALESCE(a.requires_expiration, 0) AS requires_expiration,
           a.shelf_life_days,
           COALESCE(a.requires_batch_tracking, 0) AS requires_batch_tracking,
           a.barcode,
           COALESCE(a.requires_serial_tracking, 0) AS requires_serial_tracking,
           a.is_active, a.row_version, a.created_at, a.updated_at,
           CAST(COALESCE(sb.on_hand_qty, 0) AS REAL) AS on_hand_qty,
           CAST(COALESCE(sb.reserved_qty, 0) AS REAL) AS reserved_qty,
           CAST(COALESCE(sb.available_qty, 0) AS REAL) AS available_qty,
           sb.stock_value, sb.valued_qty, sb.avg_cost_known,
           CAST(COALESCE(sb.unvalued_qty, 0) AS REAL) AS unvalued_qty
    FROM articles a
    LEFT JOIN article_families af ON af.id = a.family_id
    JOIN reference_values u ON u.id = a.unit_value_id
    LEFT JOIN lookup_values c ON c.id = a.criticality_value_id
    LEFT JOIN lookup_values st ON st.id = a.stocking_type_value_id
    LEFT JOIN lookup_values tx ON tx.id = a.tax_category_value_id
    LEFT JOIN lookup_values pc ON pc.id = a.procurement_category_value_id
    LEFT JOIN warehouses pw ON pw.id = a.preferred_warehouse_id
    LEFT JOIN stock_locations pl ON pl.id = a.preferred_location_id
    LEFT JOIN (
        SELECT article_id,
               SUM(on_hand_qty) AS on_hand_qty,
               SUM(reserved_qty) AS reserved_qty,
               SUM(available_qty) AS available_qty,
               SUM(CASE WHEN moving_avg_unit_cost IS NOT NULL
                        THEN on_hand_qty * moving_avg_unit_cost END) AS stock_value,
               SUM(CASE WHEN moving_avg_unit_cost IS NOT NULL THEN on_hand_qty END) AS valued_qty,
               AVG(moving_avg_unit_cost) AS avg_cost_known,
               SUM(CASE WHEN moving_avg_unit_cost IS NULL AND on_hand_qty > 0
                        THEN on_hand_qty ELSE 0 END) AS unvalued_qty
        FROM stock_balances
        GROUP BY article_id
    ) sb ON sb.article_id = a.id
"#;

/// Article list query with the symbol of the stock unit (`reference::units::symbol_sql`).
fn article_list_select() -> String {
    ARTICLE_LIST_SELECT_TEMPLATE.replace("{UNIT_SYMBOL}", &crate::reference::units::symbol_sql("u"))
}

pub async fn list_articles(
    db: &DatabaseConnection,
    filter: InventoryArticleFilter,
) -> AppResult<Vec<InventoryArticle>> {
    let article_list_select = article_list_select();
    let triggers = reorder_triggers_by_article(db).await?;
    let rows = if let Some(search) = filter.search {
        let q = format!("%{}%", search.trim().to_lowercase());
        db.query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!(
                "{article_list_select} \
                 WHERE LOWER(a.article_code) LIKE ? OR LOWER(a.article_name) LIKE ? \
                 ORDER BY a.article_code ASC"
            ),
            [q.clone().into(), q.into()],
        ))
        .await?
    } else {
        db.query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!("{article_list_select} ORDER BY a.article_code ASC"),
        ))
        .await?
    };

    rows.into_iter()
        .map(|row| {
            let stock_value: Option<f64> = row.try_get("", "stock_value").ok().flatten();
            let valued_qty: Option<f64> = row.try_get("", "valued_qty").ok().flatten();
            // Weighted by quantity on hand; an article valued but out of stock keeps the plain
            // mean of its location averages so the last known cost stays visible.
            let avg_unit_cost = match (stock_value, valued_qty) {
                (Some(value), Some(qty)) if qty > f64::EPSILON => Some(value / qty),
                _ => row.try_get::<Option<f64>>("", "avg_cost_known").ok().flatten(),
            };
            Ok(InventoryArticle {
                id: row.try_get("", "id")?,
                article_code: row.try_get("", "article_code")?,
                article_name: row.try_get("", "article_name")?,
                family_id: row.try_get("", "family_id")?,
                family_code: row.try_get("", "family_code")?,
                family_name: row.try_get("", "family_name")?,
                unit_value_id: row.try_get("", "unit_value_id")?,
                unit_code: row.try_get("", "unit_code")?,
                unit_label: row.try_get("", "unit_label")?,
                unit_symbol: row.try_get("", "unit_symbol")?,
                criticality_value_id: row.try_get("", "criticality_value_id")?,
                criticality_code: row.try_get("", "criticality_code")?,
                criticality_label: row.try_get("", "criticality_label")?,
                stocking_type_value_id: row.try_get("", "stocking_type_value_id")?,
                stocking_type_code: row.try_get("", "stocking_type_code")?,
                stocking_type_label: row.try_get("", "stocking_type_label")?,
                tax_category_value_id: row.try_get("", "tax_category_value_id")?,
                tax_category_code: row.try_get("", "tax_category_code")?,
                tax_category_label: row.try_get("", "tax_category_label")?,
                procurement_category_value_id: row.try_get("", "procurement_category_value_id")?,
                procurement_category_code: row.try_get("", "procurement_category_code")?,
                procurement_category_label: row.try_get("", "procurement_category_label")?,
                preferred_warehouse_id: row.try_get("", "preferred_warehouse_id")?,
                preferred_warehouse_code: row.try_get("", "preferred_warehouse_code")?,
                preferred_warehouse_name: row.try_get("", "preferred_warehouse_name")?,
                preferred_location_id: row.try_get("", "preferred_location_id")?,
                preferred_location_code: row.try_get("", "preferred_location_code")?,
                preferred_location_name: row.try_get("", "preferred_location_name")?,
                min_stock: row.try_get("", "min_stock")?,
                max_stock: row.try_get("", "max_stock")?,
                reorder_point: row.try_get("", "reorder_point")?,
                safety_stock: row.try_get("", "safety_stock")?,
                manufacturer_name: row.try_get("", "manufacturer_name").ok().flatten(),
                manufacturer_part_number: row.try_get("", "manufacturer_part_number").ok().flatten(),
                oem_part_number: row.try_get("", "oem_part_number").ok().flatten(),
                replenishment_policy_code: row.try_get("", "replenishment_policy_code").ok().flatten(),
                economic_order_qty: row.try_get("", "economic_order_qty").ok().flatten(),
                minimum_order_qty: row.try_get("", "minimum_order_qty").ok().flatten(),
                maximum_order_qty: row.try_get("", "maximum_order_qty").ok().flatten(),
                order_multiple_qty: row.try_get("", "order_multiple_qty").ok().flatten(),
                lead_time_days: row.try_get("", "lead_time_days").ok().flatten(),
                review_period_days: row.try_get("", "review_period_days").ok().flatten(),
                abc_class_code: row.try_get("", "abc_class_code").ok().flatten(),
                xyz_class_code: row.try_get("", "xyz_class_code").ok().flatten(),
                is_critical_spare: row.try_get("", "is_critical_spare").unwrap_or(0),
                requires_expiration: row.try_get("", "requires_expiration").unwrap_or(0),
                shelf_life_days: row.try_get("", "shelf_life_days").ok().flatten(),
                requires_batch_tracking: row.try_get("", "requires_batch_tracking").unwrap_or(0),
                barcode: row.try_get("", "barcode").ok().flatten(),
                requires_serial_tracking: row.try_get("", "requires_serial_tracking").unwrap_or(0),
                is_active: row.try_get("", "is_active")?,
                row_version: row.try_get("", "row_version")?,
                created_at: row.try_get("", "created_at")?,
                updated_at: row.try_get("", "updated_at")?,
                on_hand_qty: row.try_get("", "on_hand_qty")?,
                reserved_qty: row.try_get("", "reserved_qty")?,
                available_qty: row.try_get("", "available_qty")?,
                avg_unit_cost,
                stock_value,
                unvalued_qty: row.try_get("", "unvalued_qty")?,
                reorder_trigger: None,
            })
        })
        .collect::<AppResult<Vec<InventoryArticle>>>()
        .map(|mut articles| {
            for a in &mut articles {
                a.reorder_trigger = triggers.get(&a.id).cloned();
            }
            articles
        })
}

/// Articles that need replenishing in at least one store → trigger of the first such store
/// (`evaluate_reorder`, the one low-stock definition: dashboard tiles, alerts, article list).
pub async fn reorder_triggers_by_article(db: &DatabaseConnection) -> AppResult<std::collections::HashMap<i64, String>> {
    let mut map = std::collections::HashMap::new();
    for r in evaluate_reorder(db, None).await? {
        map.entry(r.article_id).or_insert(r.trigger_type);
    }
    Ok(map)
}

pub async fn create_article(db: &DatabaseConnection, input: InventoryArticleInput) -> AppResult<InventoryArticle> {
    // Stock unit: an active unit of the governed list MEASURE.UNITS.
    crate::reference::units::ensure_unit(db, input.unit_value_id, "unit_value_id", true).await?;
    if let Some(criticality_id) = input.criticality_value_id {
        ensure_lookup_value_in_domain(db, criticality_id, "equipment.criticality", "criticality_value_id").await?;
    }
    ensure_lookup_value_in_domain(
        db,
        input.stocking_type_value_id,
        "inventory.stocking_type",
        "stocking_type_value_id",
    )
    .await?;
    ensure_lookup_value_in_domain(
        db,
        input.tax_category_value_id,
        "inventory.tax_category",
        "tax_category_value_id",
    )
    .await?;
    if let Some(procurement_category_id) = input.procurement_category_value_id {
        ensure_lookup_value_in_domain(
            db,
            procurement_category_id,
            "inventory.procurement_category",
            "procurement_category_value_id",
        )
        .await?;
    }
    if let Some(family_id) = input.family_id {
        ensure_article_family_active(db, family_id).await?;
    }
    ensure_preferred_location_hint(db, input.preferred_warehouse_id, input.preferred_location_id).await?;

    if input.article_code.trim().is_empty() || input.article_name.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Article code and name are required.".to_string()
        ]));
    }
    validate_article_stock_contract(&input)?;

    let code = input.article_code.trim().to_uppercase();
    let name = input.article_name.trim().to_string();
    let is_active = parse_bool_to_i64(input.is_active, true);

    let existing = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM articles WHERE article_code = ?",
            [code.clone().into()],
        ))
        .await?;
    if existing.is_some() {
        return Err(AppError::ValidationFailed(vec![
            "Article code already exists.".to_string()
        ]));
    }

    let tx = db.begin().await?;
    let entity_sync_id = crate::sync::identity::new_entity_sync_id();
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO articles (
            entity_sync_id, article_code, article_name, family_id, unit_value_id, criticality_value_id,
            stocking_type_value_id, tax_category_value_id, procurement_category_value_id,
            preferred_warehouse_id, preferred_location_id,
            min_stock, max_stock, reorder_point, safety_stock,
            manufacturer_name, manufacturer_part_number, oem_part_number,
            replenishment_policy_code, economic_order_qty, minimum_order_qty, maximum_order_qty,
            order_multiple_qty, lead_time_days, review_period_days,
            abc_class_code, xyz_class_code,
            is_critical_spare, requires_expiration, shelf_life_days, requires_batch_tracking,
            barcode, requires_serial_tracking,
            is_active, row_version
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
        [
            entity_sync_id.into(),
            code.clone().into(),
            name.into(),
            input.family_id.map_or(Value::BigInt(None), Value::from),
            input.unit_value_id.into(),
            input.criticality_value_id.map_or(Value::BigInt(None), Value::from),
            input.stocking_type_value_id.into(),
            input.tax_category_value_id.into(),
            input
                .procurement_category_value_id
                .map_or(Value::BigInt(None), Value::from),
            input.preferred_warehouse_id.map_or(Value::BigInt(None), Value::from),
            input.preferred_location_id.map_or(Value::BigInt(None), Value::from),
            input.min_stock.into(),
            input.max_stock.map_or(Value::Double(None), Value::from),
            input.reorder_point.into(),
            input.safety_stock.into(),
            input.manufacturer_name.clone().map_or(Value::String(None), Value::from),
            input
                .manufacturer_part_number
                .clone()
                .map_or(Value::String(None), Value::from),
            input.oem_part_number.clone().map_or(Value::String(None), Value::from),
            input
                .replenishment_policy_code
                .clone()
                .map_or(Value::String(None), Value::from),
            input.economic_order_qty.map_or(Value::Double(None), Value::from),
            input.minimum_order_qty.map_or(Value::Double(None), Value::from),
            input.maximum_order_qty.map_or(Value::Double(None), Value::from),
            input.order_multiple_qty.map_or(Value::Double(None), Value::from),
            input.lead_time_days.map_or(Value::BigInt(None), Value::from),
            input.review_period_days.map_or(Value::BigInt(None), Value::from),
            input.abc_class_code.clone().map_or(Value::String(None), Value::from),
            input.xyz_class_code.clone().map_or(Value::String(None), Value::from),
            (input.is_critical_spare.unwrap_or(false) as i64).into(),
            (input.requires_expiration.unwrap_or(false) as i64).into(),
            input.shelf_life_days.map_or(Value::BigInt(None), Value::from),
            (input.requires_batch_tracking.unwrap_or(false) as i64).into(),
            normalize_barcode(input.barcode.as_deref()).map_or(Value::String(None), Value::from),
            (input.requires_serial_tracking.unwrap_or(false) as i64).into(),
            is_active.into(),
        ],
    ))
    .await?;

    let id_row = tx
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("failed to read created article id")))?;
    let article_id: i64 = id_row.try_get("", "id")?;

    record_state_event(
        &tx,
        "ARTICLE",
        article_id,
        None,
        "CREATED",
        None,
        Some("article.created"),
        Some(&format!("Article {code} created")),
    )
    .await?;

    tx.commit().await?;

    let mut rows = list_articles(
        db,
        InventoryArticleFilter {
            search: Some(code.clone()),
        },
    )
    .await?;

    rows.retain(|r| r.id == article_id);
    rows.into_iter()
        .next()
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("created article not found")))
}

pub async fn update_article(
    db: &DatabaseConnection,
    article_id: i64,
    expected_row_version: i64,
    input: InventoryArticleInput,
) -> AppResult<InventoryArticle> {
    // Stock unit: a new unit must be active in MEASURE.UNITS; the unit the article already has is
    // kept even if the plant retired it since.
    let current_unit: Option<i64> = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT unit_value_id FROM articles WHERE id = ?",
            [article_id.into()],
        ))
        .await?
        .map(|r| r.try_get::<i64>("", "unit_value_id"))
        .transpose()?;
    crate::reference::units::ensure_unit(
        db,
        input.unit_value_id,
        "unit_value_id",
        current_unit != Some(input.unit_value_id),
    )
    .await?;
    if let Some(criticality_id) = input.criticality_value_id {
        ensure_lookup_value_in_domain(db, criticality_id, "equipment.criticality", "criticality_value_id").await?;
    }
    ensure_lookup_value_in_domain(
        db,
        input.stocking_type_value_id,
        "inventory.stocking_type",
        "stocking_type_value_id",
    )
    .await?;
    ensure_lookup_value_in_domain(
        db,
        input.tax_category_value_id,
        "inventory.tax_category",
        "tax_category_value_id",
    )
    .await?;
    if let Some(procurement_category_id) = input.procurement_category_value_id {
        ensure_lookup_value_in_domain(
            db,
            procurement_category_id,
            "inventory.procurement_category",
            "procurement_category_value_id",
        )
        .await?;
    }
    if let Some(family_id) = input.family_id {
        ensure_article_family_active(db, family_id).await?;
    }
    ensure_preferred_location_hint(db, input.preferred_warehouse_id, input.preferred_location_id).await?;
    validate_article_stock_contract(&input)?;

    let code = input.article_code.trim().to_uppercase();
    let name = input.article_name.trim().to_string();
    if code.is_empty() || name.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Article code and name are required.".to_string()
        ]));
    }
    let is_active = parse_bool_to_i64(input.is_active, true);

    let before_code_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT article_code FROM articles WHERE id = ?",
            [article_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "InventoryArticle".to_string(),
            id: article_id.to_string(),
        })?;
    let before_code: String = before_code_row.try_get("", "article_code")?;
    let before = list_articles(
        db,
        InventoryArticleFilter {
            search: Some(before_code.clone()),
        },
    )
    .await?
    .into_iter()
    .find(|a| a.id == article_id)
    .ok_or_else(|| AppError::NotFound {
        entity: "InventoryArticle".to_string(),
        id: article_id.to_string(),
    })?;

    let conflicting = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM articles WHERE article_code = ? AND id <> ?",
            [code.clone().into(), article_id.into()],
        ))
        .await?;
    if conflicting.is_some() {
        return Err(AppError::ValidationFailed(vec![
            "Article code already exists.".to_string()
        ]));
    }

    let tx = db.begin().await?;
    let result = tx
        .execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE articles
             SET article_code = ?,
                 article_name = ?,
                 family_id = ?,
                 unit_value_id = ?,
                 criticality_value_id = ?,
                 stocking_type_value_id = ?,
                 tax_category_value_id = ?,
                 procurement_category_value_id = ?,
                 preferred_warehouse_id = ?,
                 preferred_location_id = ?,
                 min_stock = ?,
                 max_stock = ?,
                 reorder_point = ?,
                 safety_stock = ?,
                 manufacturer_name = ?,
                 manufacturer_part_number = ?,
                 oem_part_number = ?,
                 replenishment_policy_code = ?,
                 economic_order_qty = ?,
                 minimum_order_qty = ?,
                 maximum_order_qty = ?,
                 order_multiple_qty = ?,
                 lead_time_days = ?,
                 review_period_days = ?,
                 abc_class_code = ?,
                 xyz_class_code = ?,
                 is_critical_spare = ?,
                 requires_expiration = ?,
                 shelf_life_days = ?,
                 requires_batch_tracking = ?,
                 barcode = ?,
                 requires_serial_tracking = ?,
                 is_active = ?,
                 row_version = row_version + 1,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE id = ? AND row_version = ?",
            [
                code.clone().into(),
                name.clone().into(),
                input.family_id.map_or(Value::BigInt(None), Value::from),
                input.unit_value_id.into(),
                input.criticality_value_id.map_or(Value::BigInt(None), Value::from),
                input.stocking_type_value_id.into(),
                input.tax_category_value_id.into(),
                input
                    .procurement_category_value_id
                    .map_or(Value::BigInt(None), Value::from),
                input.preferred_warehouse_id.map_or(Value::BigInt(None), Value::from),
                input.preferred_location_id.map_or(Value::BigInt(None), Value::from),
                input.min_stock.into(),
                input.max_stock.map_or(Value::Double(None), Value::from),
                input.reorder_point.into(),
                input.safety_stock.into(),
                input.manufacturer_name.clone().map_or(Value::String(None), Value::from),
                input
                    .manufacturer_part_number
                    .clone()
                    .map_or(Value::String(None), Value::from),
                input.oem_part_number.clone().map_or(Value::String(None), Value::from),
                input
                    .replenishment_policy_code
                    .clone()
                    .map_or(Value::String(None), Value::from),
                input.economic_order_qty.map_or(Value::Double(None), Value::from),
                input.minimum_order_qty.map_or(Value::Double(None), Value::from),
                input.maximum_order_qty.map_or(Value::Double(None), Value::from),
                input.order_multiple_qty.map_or(Value::Double(None), Value::from),
                input.lead_time_days.map_or(Value::BigInt(None), Value::from),
                input.review_period_days.map_or(Value::BigInt(None), Value::from),
                input.abc_class_code.clone().map_or(Value::String(None), Value::from),
                input.xyz_class_code.clone().map_or(Value::String(None), Value::from),
                (input.is_critical_spare.unwrap_or(false) as i64).into(),
                (input.requires_expiration.unwrap_or(false) as i64).into(),
                input.shelf_life_days.map_or(Value::BigInt(None), Value::from),
                (input.requires_batch_tracking.unwrap_or(false) as i64).into(),
                normalize_barcode(input.barcode.as_deref()).map_or(Value::String(None), Value::from),
                (input.requires_serial_tracking.unwrap_or(false) as i64).into(),
                is_active.into(),
                article_id.into(),
                expected_row_version.into(),
            ],
        ))
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::ValidationFailed(vec![
            "Article update failed (not found or stale row_version).".to_string(),
        ]));
    }

    record_article_field_changes(&tx, article_id, &before, &input, &code, &name, is_active).await?;

    tx.commit().await?;

    let rows = list_articles(
        db,
        InventoryArticleFilter {
            search: Some(code.clone()),
        },
    )
    .await?;

    rows.into_iter()
        .find(|r| r.id == article_id)
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("updated article not found")))
}

async fn record_article_field_changes<C: ConnectionTrait>(
    db: &C,
    article_id: i64,
    before: &InventoryArticle,
    input: &InventoryArticleInput,
    new_code: &str,
    new_name: &str,
    new_is_active: i64,
) -> AppResult<()> {
    let mut changes: Vec<(&str, String)> = Vec::new();

    if before.article_code != new_code {
        changes.push(("field.article_code", format!("{} → {}", before.article_code, new_code)));
    }
    if before.article_name != new_name {
        changes.push(("field.article_name", format!("{} → {}", before.article_name, new_name)));
    }
    if before.family_id != input.family_id {
        let from = before
            .family_name
            .clone()
            .or_else(|| before.family_code.clone())
            .unwrap_or_else(|| "—".to_string());
        changes.push(("field.family", format!("{from} → updated")));
    }
    if before.unit_value_id != input.unit_value_id {
        changes.push(("field.unit", format!("{} → updated", before.unit_label)));
    }
    if before.criticality_value_id != input.criticality_value_id {
        let from = before
            .criticality_label
            .clone()
            .or_else(|| before.criticality_code.clone())
            .unwrap_or_else(|| "—".to_string());
        changes.push(("field.criticality", format!("{from} → updated")));
    }
    if before.stocking_type_value_id != input.stocking_type_value_id {
        changes.push((
            "field.stocking_type",
            format!("{} → updated", before.stocking_type_label),
        ));
    }
    if before.tax_category_value_id != input.tax_category_value_id {
        changes.push(("field.tax_category", format!("{} → updated", before.tax_category_label)));
    }
    if before.procurement_category_value_id != input.procurement_category_value_id {
        let from = before
            .procurement_category_label
            .clone()
            .or_else(|| before.procurement_category_code.clone())
            .unwrap_or_else(|| "—".to_string());
        changes.push(("field.procurement_category", format!("{from} → updated")));
    }
    if before.preferred_warehouse_id != input.preferred_warehouse_id {
        let from = before
            .preferred_warehouse_code
            .clone()
            .unwrap_or_else(|| "—".to_string());
        changes.push(("field.preferred_warehouse", format!("{from} → updated")));
    }
    if before.preferred_location_id != input.preferred_location_id {
        let from = before
            .preferred_location_code
            .clone()
            .unwrap_or_else(|| "—".to_string());
        changes.push(("field.preferred_location", format!("{from} → updated")));
    }
    if (before.min_stock - input.min_stock).abs() > f64::EPSILON {
        changes.push(("field.min_stock", format!("{} → {}", before.min_stock, input.min_stock)));
    }
    if before.max_stock != input.max_stock {
        changes.push((
            "field.max_stock",
            format!("{} → {}", opt_f64(before.max_stock), opt_f64(input.max_stock)),
        ));
    }
    if (before.reorder_point - input.reorder_point).abs() > f64::EPSILON {
        changes.push((
            "field.reorder_point",
            format!("{} → {}", before.reorder_point, input.reorder_point),
        ));
    }
    if (before.safety_stock - input.safety_stock).abs() > f64::EPSILON {
        changes.push((
            "field.safety_stock",
            format!("{} → {}", before.safety_stock, input.safety_stock),
        ));
    }
    if before.is_active != new_is_active {
        changes.push(("field.is_active", format!("{} → {}", before.is_active, new_is_active)));
    }

    if changes.is_empty() {
        return Ok(());
    }

    for (reason, note) in changes {
        record_state_event(
            db,
            "ARTICLE",
            article_id,
            Some("UPDATED"),
            "UPDATED",
            None,
            Some(reason),
            Some(&note),
        )
        .await?;
    }
    Ok(())
}

/// Barcodes are compared as typed by a scanner: surrounding spaces removed, empty means none.
fn normalize_barcode(value: Option<&str>) -> Option<String> {
    value.map(str::trim).filter(|v| !v.is_empty()).map(str::to_string)
}

fn opt_f64(v: Option<f64>) -> String {
    v.map(|n| n.to_string()).unwrap_or_else(|| "—".to_string())
}

pub async fn list_stock_balances(
    db: &DatabaseConnection,
    filter: InventoryStockFilter,
) -> AppResult<Vec<InventoryStockBalance>> {
    let mut sql = String::from(
        r#"
        SELECT sb.id, sb.article_id, a.article_code, a.article_name,
               sb.warehouse_id, w.code AS warehouse_code, w.name AS warehouse_name,
               sb.location_id, sl.code AS location_code, sl.name AS location_name,
               sb.on_hand_qty, sb.reserved_qty, sb.available_qty, sb.moving_avg_unit_cost, sb.updated_at
        FROM stock_balances sb
        JOIN articles a ON a.id = sb.article_id
        JOIN warehouses w ON w.id = sb.warehouse_id
        JOIN stock_locations sl ON sl.id = sb.location_id
        WHERE 1=1
        "#,
    );

    let mut values = Vec::new();

    if let Some(article_id) = filter.article_id {
        sql.push_str(" AND sb.article_id = ?");
        values.push(article_id.into());
    }
    if let Some(warehouse_id) = filter.warehouse_id {
        sql.push_str(" AND sb.warehouse_id = ?");
        values.push(warehouse_id.into());
    }
    // "Low stock only" is the one low-stock rule (`evaluate_reorder`: per article and store, on
    // the store's available quantity against the reorder point or the minimum stock), applied to
    // the rows below — not a per-location comparison of its own.
    sql.push_str(" ORDER BY a.article_code ASC, w.code ASC, sl.code ASC");

    let rows = if values.is_empty() {
        db.query_all(Statement::from_string(DbBackend::Sqlite, sql)).await?
    } else {
        db.query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
            .await?
    };

    let mut balances: Vec<InventoryStockBalance> = rows
        .into_iter()
        .map(|row| {
            Ok(InventoryStockBalance {
                id: row.try_get("", "id")?,
                article_id: row.try_get("", "article_id")?,
                article_code: row.try_get("", "article_code")?,
                article_name: row.try_get("", "article_name")?,
                warehouse_id: row.try_get("", "warehouse_id")?,
                warehouse_code: row.try_get("", "warehouse_code")?,
                warehouse_name: row.try_get("", "warehouse_name")?,
                location_id: row.try_get("", "location_id")?,
                location_code: row.try_get("", "location_code")?,
                location_name: row.try_get("", "location_name")?,
                on_hand_qty: row.try_get("", "on_hand_qty")?,
                reserved_qty: row.try_get("", "reserved_qty")?,
                available_qty: row.try_get("", "available_qty")?,
                moving_avg_unit_cost: row.try_get("", "moving_avg_unit_cost").ok().flatten(),
                updated_at: row.try_get("", "updated_at")?,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;

    // Articles without a `stock_balances` row (e.g. newly created) still appear with zero qty so
    // the Stock balances UI is not empty until the first receipt/adjustment.
    let mut syn_sql = String::from(
        r#"
        SELECT
          -(a.id * 1000000000 + sl.id) AS id,
          a.id AS article_id,
          a.article_code,
          a.article_name,
          sl.warehouse_id,
          w.code AS warehouse_code,
          w.name AS warehouse_name,
          sl.id AS location_id,
          sl.code AS location_code,
          sl.name AS location_name,
          0.0 AS on_hand_qty,
          0.0 AS reserved_qty,
          0.0 AS available_qty,
          NULL AS moving_avg_unit_cost,
          strftime('%Y-%m-%dT%H:%M:%SZ','now') AS updated_at
        FROM articles a
        INNER JOIN stock_locations sl ON sl.id = (
          CASE
            WHEN a.preferred_location_id IS NOT NULL
                 AND EXISTS (
                   SELECT 1 FROM stock_locations sl_pref
                   WHERE sl_pref.id = a.preferred_location_id AND sl_pref.is_active = 1
                 )
              THEN a.preferred_location_id
            ELSE (
              SELECT id FROM stock_locations
              WHERE is_active = 1
              ORDER BY warehouse_id, is_default DESC, id
              LIMIT 1
            )
          END
        )
        INNER JOIN warehouses w ON w.id = sl.warehouse_id
        WHERE a.is_active = 1
          AND sl.is_active = 1
          AND w.is_active = 1
          AND NOT EXISTS (
            SELECT 1 FROM stock_balances sb
            WHERE sb.article_id = a.id AND sb.location_id = sl.id
          )
        "#,
    );

    let mut syn_values = Vec::new();
    if let Some(article_id) = filter.article_id {
        syn_sql.push_str(" AND a.id = ?");
        syn_values.push(article_id.into());
    }
    if let Some(warehouse_id) = filter.warehouse_id {
        syn_sql.push_str(" AND sl.warehouse_id = ?");
        syn_values.push(warehouse_id.into());
    }

    let syn_rows = if syn_values.is_empty() {
        db.query_all(Statement::from_string(DbBackend::Sqlite, syn_sql)).await?
    } else {
        db.query_all(Statement::from_sql_and_values(DbBackend::Sqlite, syn_sql, syn_values))
            .await?
    };

    for row in syn_rows {
        balances.push(InventoryStockBalance {
            id: row.try_get("", "id")?,
            article_id: row.try_get("", "article_id")?,
            article_code: row.try_get("", "article_code")?,
            article_name: row.try_get("", "article_name")?,
            warehouse_id: row.try_get("", "warehouse_id")?,
            warehouse_code: row.try_get("", "warehouse_code")?,
            warehouse_name: row.try_get("", "warehouse_name")?,
            location_id: row.try_get("", "location_id")?,
            location_code: row.try_get("", "location_code")?,
            location_name: row.try_get("", "location_name")?,
            on_hand_qty: row.try_get("", "on_hand_qty")?,
            reserved_qty: row.try_get("", "reserved_qty")?,
            available_qty: row.try_get("", "available_qty")?,
            moving_avg_unit_cost: row.try_get("", "moving_avg_unit_cost").ok().flatten(),
            updated_at: row.try_get("", "updated_at")?,
        });
    }

    if filter.low_stock_only.unwrap_or(false) {
        let short: std::collections::HashSet<(i64, i64)> = evaluate_reorder(db, filter.warehouse_id)
            .await?
            .into_iter()
            .map(|r| (r.article_id, r.warehouse_id))
            .collect();
        balances.retain(|b| short.contains(&(b.article_id, b.warehouse_id)));
    }

    balances.sort_by(|a, b| {
        a.article_code
            .cmp(&b.article_code)
            .then_with(|| a.warehouse_code.cmp(&b.warehouse_code))
            .then_with(|| a.location_code.cmp(&b.location_code))
    });

    Ok(balances)
}

/// Movement reasons that move parts to or from a work order (`inventory.movement_type` lookup).
const WORK_ORDER_MOVEMENT_REASONS: &[&str] = &["SORTIE_OT", "RETOUR_OT"];

/// A manual exit to / return from a work order must name an existing work order (its code is
/// kept as the movement's source reference), otherwise the parts cost cannot be traced.
async fn ensure_work_order_reference(
    db: &DatabaseConnection,
    reason_code: Option<&str>,
    source_ref: Option<&str>,
) -> AppResult<()> {
    let Some(reason) = reason_code else {
        return Ok(());
    };
    if !WORK_ORDER_MOVEMENT_REASONS.contains(&reason) {
        return Ok(());
    }
    let wo_code = source_ref.map(str::trim).unwrap_or("");
    if wo_code.is_empty() {
        return Err(AppError::ValidationFailed(vec![format!(
            "Movement reason '{reason}' requires a work order reference."
        )]));
    }
    let exists = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM work_orders WHERE code = ?",
            [wo_code.to_string().into()],
        ))
        .await?;
    if exists.is_none() {
        return Err(AppError::ValidationFailed(vec![format!(
            "Work order '{wo_code}' does not exist."
        )]));
    }
    Ok(())
}

pub async fn adjust_stock(
    db: &DatabaseConnection,
    input: InventoryStockAdjustInput,
) -> AppResult<InventoryStockBalance> {
    adjust_stock_costed(db, input, None).await
}

/// Manual stock movement. An entry may carry its `unit_cost` (tenant currency): the location's
/// weighted-average cost is recomputed and the movement is stamped with that cost. Entries without
/// a cost and all exits are valued at the location's current average (when it has one).
pub async fn adjust_stock_costed(
    db: &DatabaseConnection,
    input: InventoryStockAdjustInput,
    unit_cost: Option<f64>,
) -> AppResult<InventoryStockBalance> {
    adjust_stock_full(
        db,
        input,
        AdjustStockOptions {
            unit_cost,
            ..Default::default()
        },
    )
    .await
}

/// Extras of a manual movement: entry cost, lot (existing or new) and a back-dated posting date.
#[derive(Debug, Clone, Default)]
pub struct AdjustStockOptions {
    pub unit_cost: Option<f64>,
    /// Existing lot to move (exits; entries into a known lot).
    pub lot_id: Option<i64>,
    /// New lot / serial for an entry of a tracked article.
    pub new_lot: Option<crate::inventory::domain::NewLotInput>,
    pub posting_date: Option<String>,
    pub actor_id: Option<i64>,
}

pub async fn adjust_stock_full(
    db: &DatabaseConnection,
    input: InventoryStockAdjustInput,
    options: AdjustStockOptions,
) -> AppResult<InventoryStockBalance> {
    let unit_cost = options.unit_cost;
    let posting_date = crate::inventory::periods::resolve_posting_date(db, options.posting_date.as_deref()).await?;
    let tracking = crate::inventory::lots::article_tracking(db, input.article_id).await?;
    crate::inventory::lots::ensure_whole_units(&tracking, input.delta_qty.abs())?;
    if input.delta_qty == 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "delta_qty must be different from 0.".to_string()
        ]));
    }
    if let Some(cost) = unit_cost {
        crate::inventory::valuation::validate_unit_cost(cost)?;
        if input.delta_qty < 0.0 {
            return Err(AppError::ValidationFailed(vec![
                "unit_cost applies to stock entries only; exits are valued at the average cost.".to_string(),
            ]));
        }
    }
    ensure_work_order_reference(db, input.reason_code.as_deref(), input.source_ref.as_deref()).await?;

    let tx = db.begin().await?;
    let warehouse_id = ensure_active_mutation_context(&tx, input.article_id, input.location_id).await?;
    let (on_hand, reserved) = get_balance_snapshot(&tx, input.article_id, input.location_id).await?;
    let next_on_hand = on_hand + input.delta_qty;
    // An exit never takes stock below zero; an entry at a location already below zero is accepted
    // (CMMS Advisor decision 1) and is a count resolution: its comment is the reason, at least 8
    // characters, and it is audited (decision 2, `inventory::negative_stock`).
    if !crate::inventory::negative_stock::balance_change_allowed((on_hand, reserved), (next_on_hand, reserved)) {
        return Err(AppError::ValidationFailed(vec![
            "Adjustment would produce negative stock.".to_string(),
        ]));
    }
    crate::inventory::negative_stock::check_entry_reason(on_hand, input.delta_qty, input.notes.as_deref())?;
    let reconciles_negative = input.delta_qty > 0.0 && on_hand < -crate::inventory::negative_stock::QTY_TOLERANCE;
    // An entry made here while the location stood below zero is marked as such on the movement, so
    // every PC tells it (reason asked, audited) from an entry made before the shortage was visible.
    let source_type = if reconciles_negative {
        crate::inventory::negative_stock::SOURCE_NEGATIVE_STOCK_ENTRY
    } else {
        "MANUAL_ADJUSTMENT"
    };

    // Critical spare guard: block exits that would leave on_hand = 0
    if input.delta_qty < 0.0 && next_on_hand == 0.0 {
        let critical_row = tx
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT COALESCE(is_critical_spare, 0) AS is_critical_spare FROM articles WHERE id = ?",
                [input.article_id.into()],
            ))
            .await?;
        if let Some(row) = critical_row {
            let is_critical: i64 = row.try_get("", "is_critical_spare").unwrap_or(0);
            if is_critical == 1 {
                return Err(AppError::ValidationFailed(vec![
                    "Cannot reduce on-hand to 0 for a critical spare.".to_string(),
                ]));
            }
        }
    }

    upsert_balance(
        &tx,
        input.article_id,
        warehouse_id,
        input.location_id,
        next_on_hand,
        reserved,
    )
    .await?;

    // Lot of the movement: entries of a tracked article name their lot (new or existing), exits
    // take the chosen lot or FEFO.
    let lot_allocation: Vec<(Option<i64>, f64)> = if !tracking.is_tracked() {
        vec![(None, input.delta_qty)]
    } else if input.delta_qty > 0.0 {
        if tracking.serial && (input.delta_qty - 1.0).abs() > 1e-9 {
            return Err(AppError::ValidationFailed(vec![
                "A serial number stands for exactly one unit: enter serialised items one at a time.".to_string(),
            ]));
        }
        let lot = match (options.lot_id, options.new_lot.as_ref()) {
            (Some(lot), _) => lot,
            (None, Some(new_lot)) => {
                crate::inventory::lots::create_or_reuse_lot(&tx, input.article_id, new_lot, None, options.actor_id)
                    .await?
            }
            (None, None) => {
                return Err(AppError::ValidationFailed(vec![
                    "This article is lot/serial-tracked: give the lot or serial number of the entry.".to_string(),
                ]))
            }
        };
        vec![(Some(lot), input.delta_qty)]
    } else {
        let qty = input.delta_qty.abs();
        match options.lot_id {
            Some(lot) => {
                crate::inventory::lots::ensure_lot_available(&tx, lot, input.article_id, input.location_id, qty)
                    .await?;
                vec![(Some(lot), input.delta_qty)]
            }
            None => crate::inventory::lots::fefo_allocate(&tx, input.article_id, input.location_id, qty)
                .await?
                .into_iter()
                .map(|(lot, q)| (Some(lot), -q))
                .collect(),
        }
    };
    let mut transaction_id: Option<i64> = None;
    for (lot_id, delta) in lot_allocation {
        let movement_id = insert_inventory_transaction(
            &tx,
            input.article_id,
            warehouse_id,
            input.location_id,
            None,
            if delta >= 0.0 { "ADJUST_IN" } else { "ADJUST_OUT" },
            delta.abs(),
            source_type,
            None,
            input.source_ref.as_deref(),
            input.reason_code.as_deref(),
            input.notes.as_deref(),
            None,
        )
        .await?;
        if let Some(lot) = lot_id {
            crate::inventory::lots::set_movement_lot(&tx, movement_id, lot).await?;
        }
        crate::inventory::periods::set_movement_posting_date(&tx, movement_id, &posting_date).await?;
        set_movement_actor(&tx, movement_id, options.actor_id).await?;
        if unit_cost.is_none() {
            // Exits (possibly split over lots) and uncosted entries: valued at the average.
            crate::inventory::valuation::stamp_at_average(&tx, input.article_id, input.location_id, delta, movement_id)
                .await?;
        }
        transaction_id = Some(movement_id);
    }
    let transaction_id = transaction_id.expect("at least one movement");

    // A costed entry is a single movement (one lot at most): it updates the weighted average.
    if let Some(cost) = unit_cost {
        crate::inventory::valuation::apply_inbound_cost(
            &tx,
            input.article_id,
            input.location_id,
            on_hand,
            input.delta_qty,
            cost,
            transaction_id,
            crate::inventory::valuation::COST_SOURCE_MANUAL_ENTRY,
            input.source_ref.as_deref(),
        )
        .await?;
    }

    if reconciles_negative {
        crate::inventory::negative_stock::reconcile_pair_in(&tx, input.article_id, input.location_id).await?;
        crate::audit::writer::write_audit_event_in(
            &tx,
            crate::audit::writer::AuditEventInput {
                action_code: "inventory.negative_stock.adjusted".to_string(),
                target_type: Some("stock_location_article".to_string()),
                target_id: Some(format!("{}:{}", input.article_id, input.location_id)),
                actor_id: options.actor_id,
                auth_context: "session".to_string(),
                result: "success".to_string(),
                before_hash: None,
                after_hash: None,
                retention_class: "standard".to_string(),
                details_json: Some(serde_json::json!({
                    "summary": "Manual stock entry at a location below zero",
                    "article_id": input.article_id,
                    "location_id": input.location_id,
                    "on_hand_before": on_hand,
                    "delta_qty": input.delta_qty,
                    "on_hand_after": next_on_hand,
                    "reason_code": input.reason_code,
                    "reason": input.notes.as_deref().map(str::trim),
                    "transaction_id": transaction_id,
                })),
            },
        )
        .await?;
    }

    tx.commit().await?;
    load_balance(db, input.article_id, input.location_id).await
}

async fn ensure_location_exists<C: ConnectionTrait>(db: &C, location_id: i64) -> AppResult<i64> {
    let location_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT sl.warehouse_id, sl.is_active AS location_active, w.is_active AS warehouse_active
             FROM stock_locations sl
             JOIN warehouses w ON w.id = sl.warehouse_id
             WHERE sl.id = ?",
            [location_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::ValidationFailed(vec!["location_id does not exist.".to_string()]))?;
    let location_active: i64 = location_row.try_get("", "location_active")?;
    if location_active == 0 {
        return Err(AppError::ValidationFailed(vec![
            "location_id must reference an active location.".to_string(),
        ]));
    }
    let warehouse_active: i64 = location_row.try_get("", "warehouse_active")?;
    if warehouse_active == 0 {
        return Err(AppError::ValidationFailed(vec![
            "location_id must belong to an active warehouse.".to_string(),
        ]));
    }
    Ok(location_row.try_get("", "warehouse_id")?)
}

async fn ensure_article_active<C: ConnectionTrait>(db: &C, article_id: i64) -> AppResult<()> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT is_active FROM articles WHERE id = ?",
            [article_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::ValidationFailed(vec!["article_id does not exist.".to_string()]))?;
    let is_active: i64 = row.try_get("", "is_active")?;
    if is_active == 0 {
        return Err(AppError::ValidationFailed(vec![
            "article_id must reference an active article.".to_string(),
        ]));
    }
    Ok(())
}

async fn ensure_active_mutation_context<C: ConnectionTrait>(
    db: &C,
    article_id: i64,
    location_id: i64,
) -> AppResult<i64> {
    ensure_article_active(db, article_id).await?;
    ensure_location_exists(db, location_id).await
}

fn ensure_reservation_invariants(reservation: &StockReservation) -> AppResult<()> {
    if reservation.quantity_reserved < 0.0 || reservation.quantity_issued < 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "Reservation quantities cannot be negative.".to_string(),
        ]));
    }
    if reservation.quantity_issued > reservation.quantity_reserved {
        return Err(AppError::ValidationFailed(vec![
            "Reservation invariant violation: quantity_issued exceeds quantity_reserved.".to_string(),
        ]));
    }
    Ok(())
}

async fn get_balance_snapshot<C: ConnectionTrait>(db: &C, article_id: i64, location_id: i64) -> AppResult<(f64, f64)> {
    let existing = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT on_hand_qty, reserved_qty
             FROM stock_balances
             WHERE article_id = ? AND location_id = ?",
            [article_id.into(), location_id.into()],
        ))
        .await?;
    if let Some(row) = existing {
        Ok((row.try_get("", "on_hand_qty")?, row.try_get("", "reserved_qty")?))
    } else {
        Ok((0.0, 0.0))
    }
}

async fn upsert_balance<C: ConnectionTrait>(
    db: &C,
    article_id: i64,
    warehouse_id: i64,
    location_id: i64,
    on_hand_qty: f64,
    reserved_qty: f64,
) -> AppResult<()> {
    // Never below zero, except a location already there may be improved (a receipt, a transfer in,
    // a return, a released reservation): `negative_stock::balance_change_allowed`.
    let current = get_balance_snapshot(db, article_id, location_id).await?;
    if !crate::inventory::negative_stock::balance_change_allowed(current, (on_hand_qty, reserved_qty)) {
        return Err(AppError::ValidationFailed(vec![
            if on_hand_qty < 0.0 || reserved_qty < 0.0 {
                "Stock balances cannot be negative.".to_string()
            } else {
                "available_qty cannot be negative.".to_string()
            },
        ]));
    }
    let available_qty = on_hand_qty - reserved_qty;

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO stock_balances (
             article_id, warehouse_id, location_id, on_hand_qty, reserved_qty, available_qty, updated_at
         ) VALUES (?, ?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now'))
         ON CONFLICT(article_id, location_id) DO UPDATE SET
             warehouse_id = excluded.warehouse_id,
             on_hand_qty = excluded.on_hand_qty,
             reserved_qty = excluded.reserved_qty,
             available_qty = excluded.available_qty,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')",
        [
            article_id.into(),
            warehouse_id.into(),
            location_id.into(),
            on_hand_qty.into(),
            reserved_qty.into(),
            available_qty.into(),
        ],
    ))
    .await?;

    Ok(())
}

async fn insert_inventory_transaction<C: ConnectionTrait>(
    db: &C,
    article_id: i64,
    warehouse_id: i64,
    location_id: i64,
    reservation_id: Option<i64>,
    movement_type: &str,
    quantity: f64,
    source_type: &str,
    source_id: Option<i64>,
    source_ref: Option<&str>,
    reason_code: Option<&str>,
    notes: Option<&str>,
    entity_sync_id: Option<uuid::Uuid>,
) -> AppResult<i64> {
    // No movement may be dated inside a closed valuation period (business date defaults to
    // today; back-dated postings are checked by their command before they reach the ledger).
    crate::inventory::periods::ensure_posting_date_open(db, &crate::inventory::fx::today()).await?;
    let entity_sync_id = entity_sync_id.unwrap_or_else(uuid::Uuid::new_v4).to_string();
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inventory_transactions (
             entity_sync_id, article_id, warehouse_id, location_id, reservation_id, movement_type, quantity,
             source_type, source_id, source_ref, reason_code, reason, performed_by_id, performed_at,
             row_version
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, strftime('%Y-%m-%dT%H:%M:%SZ','now'), 1)",
        [
            entity_sync_id.into(),
            article_id.into(),
            warehouse_id.into(),
            location_id.into(),
            reservation_id.into(),
            movement_type.to_string().into(),
            quantity.into(),
            source_type.to_string().into(),
            source_id.into(),
            source_ref.map(|v| v.to_string()).into(),
            reason_code.map(|v| v.to_string()).into(),
            notes.map(|v| v.to_string()).into(),
        ],
    ))
    .await?;
    let id_row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("failed to read inventory transaction id")))?;
    Ok(id_row.try_get("", "id")?)
}

async fn load_balance(db: &DatabaseConnection, article_id: i64, location_id: i64) -> AppResult<InventoryStockBalance> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT sb.id, sb.article_id, a.article_code, a.article_name,
                    sb.warehouse_id, w.code AS warehouse_code, w.name AS warehouse_name,
                    sb.location_id, sl.code AS location_code, sl.name AS location_name,
                    sb.on_hand_qty, sb.reserved_qty, sb.available_qty, sb.moving_avg_unit_cost, sb.updated_at
             FROM stock_balances sb
             JOIN articles a ON a.id = sb.article_id
             JOIN warehouses w ON w.id = sb.warehouse_id
             JOIN stock_locations sl ON sl.id = sb.location_id
             WHERE sb.article_id = ? AND sb.location_id = ?",
            [article_id.into(), location_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("stock balance not found after mutation")))?;

    Ok(InventoryStockBalance {
        id: row.try_get("", "id")?,
        article_id: row.try_get("", "article_id")?,
        article_code: row.try_get("", "article_code")?,
        article_name: row.try_get("", "article_name")?,
        warehouse_id: row.try_get("", "warehouse_id")?,
        warehouse_code: row.try_get("", "warehouse_code")?,
        warehouse_name: row.try_get("", "warehouse_name")?,
        location_id: row.try_get("", "location_id")?,
        location_code: row.try_get("", "location_code")?,
        location_name: row.try_get("", "location_name")?,
        on_hand_qty: row.try_get("", "on_hand_qty")?,
        reserved_qty: row.try_get("", "reserved_qty")?,
        available_qty: row.try_get("", "available_qty")?,
        moving_avg_unit_cost: row.try_get("", "moving_avg_unit_cost").ok().flatten(),
        updated_at: row.try_get("", "updated_at")?,
    })
}

async fn load_reservation_by_id<C: ConnectionTrait>(db: &C, reservation_id: i64) -> AppResult<StockReservation> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT r.id, r.article_id, a.article_code, a.article_name,
                    r.warehouse_id, w.code AS warehouse_code, w.name AS warehouse_name,
                    r.location_id, sl.code AS location_code, sl.name AS location_name,
                    r.source_type, r.source_id, r.source_ref,
                    CASE WHEN r.source_type IN ('WORK_ORDER','WORK_ORDER_PART') THEN COALESCE(wo.code, r.source_ref) ELSE NULL END AS work_order_code,
                    r.quantity_reserved, r.quantity_issued, r.status, r.notes,
                    r.created_by_id, r.created_at, r.updated_at, r.released_at
             FROM stock_reservations r
             JOIN articles a ON a.id = r.article_id
             JOIN warehouses w ON w.id = r.warehouse_id
             JOIN stock_locations sl ON sl.id = r.location_id
             LEFT JOIN work_orders wo ON wo.id = r.source_id AND r.source_type IN ('WORK_ORDER','WORK_ORDER_PART')
             WHERE r.id = ?",
            [reservation_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "StockReservation".to_string(),
            id: reservation_id.to_string(),
        })?;
    Ok(StockReservation {
        id: row.try_get("", "id")?,
        article_id: row.try_get("", "article_id")?,
        article_code: row.try_get("", "article_code")?,
        article_name: row.try_get("", "article_name")?,
        warehouse_id: row.try_get("", "warehouse_id")?,
        warehouse_code: row.try_get("", "warehouse_code")?,
        warehouse_name: row.try_get("", "warehouse_name")?,
        location_id: row.try_get("", "location_id")?,
        location_code: row.try_get("", "location_code")?,
        location_name: row.try_get("", "location_name")?,
        source_type: row.try_get("", "source_type")?,
        source_id: row.try_get("", "source_id")?,
        source_ref: row.try_get("", "source_ref")?,
        work_order_code: row.try_get("", "work_order_code").ok().flatten(),
        quantity_reserved: row.try_get("", "quantity_reserved")?,
        quantity_issued: row.try_get("", "quantity_issued")?,
        status: row.try_get("", "status")?,
        notes: row.try_get("", "notes")?,
        created_by_id: row.try_get("", "created_by_id")?,
        created_at: row.try_get("", "created_at")?,
        updated_at: row.try_get("", "updated_at")?,
        released_at: row.try_get("", "released_at")?,
    })
}

pub async fn reserve_stock(db: &DatabaseConnection, input: InventoryReserveInput) -> AppResult<StockReservation> {
    let tx = db.begin().await?;
    let reservation_id = reserve_stock_in(&tx, &input).await?;
    tx.commit().await?;
    load_reservation_by_id(db, reservation_id).await
}

/// Reserves stock inside the caller's transaction and returns the reservation id.
pub(crate) async fn reserve_stock_in<C: ConnectionTrait>(tx: &C, input: &InventoryReserveInput) -> AppResult<i64> {
    if input.quantity <= 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "quantity must be greater than 0.".to_string()
        ]));
    }
    if input.source_type.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec!["source_type is required.".to_string()]));
    }
    let warehouse_id = ensure_active_mutation_context(tx, input.article_id, input.location_id).await?;
    let (on_hand, reserved) = get_balance_snapshot(tx, input.article_id, input.location_id).await?;
    let available = on_hand - reserved;
    if available < input.quantity {
        return Err(AppError::ValidationFailed(vec![format!(
            "Insufficient available stock for reservation (available: {available})."
        )]));
    }

    upsert_balance(
        tx,
        input.article_id,
        warehouse_id,
        input.location_id,
        on_hand,
        reserved + input.quantity,
    )
    .await?;

    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO stock_reservations (
             entity_sync_id, article_id, warehouse_id, location_id, source_type, source_id, source_ref,
             quantity_reserved, quantity_issued, status, notes, created_by_id, row_version
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 0, 'active', ?, NULL, 1)",
        [
            uuid::Uuid::new_v4().to_string().into(),
            input.article_id.into(),
            warehouse_id.into(),
            input.location_id.into(),
            input.source_type.clone().into(),
            input.source_id.into(),
            input.source_ref.clone().into(),
            input.quantity.into(),
            input.notes.clone().into(),
        ],
    ))
    .await?;

    let reservation_id_row = tx
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("failed to load reservation id")))?;
    let reservation_id: i64 = reservation_id_row.try_get("", "id")?;

    insert_inventory_transaction(
        tx,
        input.article_id,
        warehouse_id,
        input.location_id,
        Some(reservation_id),
        "RESERVE",
        input.quantity,
        &input.source_type,
        input.source_id,
        input.source_ref.as_deref(),
        None,
        input.notes.as_deref(),
        None,
    )
    .await?;
    crate::inventory::reservations::recompute_reserved_in(tx, input.article_id, input.location_id).await?;
    Ok(reservation_id)
}

/// Issue against an existing reservation (WO planned / used parts). Kept for the WO module; it
/// posts through the same path as the issue desk (`inventory::issue::issue_stock`): lots picked
/// FEFO for tracked articles, cost at the weighted average, closed periods refused.
pub async fn issue_reserved_stock(db: &DatabaseConnection, input: InventoryIssueInput) -> AppResult<StockReservation> {
    let reservation = load_reservation_by_id(db, input.reservation_id).await?;
    crate::inventory::issue::issue_stock_with_source(
        db,
        crate::inventory::domain::IssueStockRequest {
            article_id: reservation.article_id,
            location_id: reservation.location_id,
            quantity: input.quantity,
            work_order_id: None,
            reservation_id: Some(reservation.id),
            lot_id: None,
            notes: input.notes.clone(),
            actor_id: None,
            // A planned part is issued in the stock unit of its reservation.
            entered_unit_value_id: None,
        },
        crate::inventory::issue::IssueSourceOverride {
            source_type: input.source_type.clone(),
            source_id: input.source_id,
            source_ref: input.source_ref.clone(),
        },
    )
    .await?;
    load_reservation_by_id(db, reservation.id).await
}

/// One ISSUE movement of `quantity` against a reservation, inside the caller's transaction.
/// Returns the movement id and its weighted-average unit cost (None when the location is not
/// valued yet).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn issue_reserved_in<C: ConnectionTrait>(
    tx: &C,
    reservation_id: i64,
    quantity: f64,
    lot_id: Option<i64>,
    source_type: Option<&str>,
    source_id: Option<i64>,
    source_ref: Option<&str>,
    notes: Option<&str>,
    actor_id: Option<i64>,
) -> AppResult<(i64, Option<f64>)> {
    if quantity <= 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "quantity must be greater than 0.".to_string()
        ]));
    }
    let reservation = load_reservation_by_id(tx, reservation_id).await?;
    ensure_reservation_invariants(&reservation)?;
    ensure_active_mutation_context(tx, reservation.article_id, reservation.location_id).await?;
    if reservation.status == "released" {
        return Err(AppError::ValidationFailed(vec![
            "Reservation already released.".to_string()
        ]));
    }
    // D13: a work order that is closed or cancelled takes no more parts, also through a
    // reservation still open on it (ended before D13, or made on another PC meanwhile).
    if let Some((wo_id, _)) =
        crate::inventory::reservations::ended_work_order_of_reservation(tx, reservation.id).await?
    {
        let wo_code = tx
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT code FROM work_orders WHERE id = ?",
                [wo_id.into()],
            ))
            .await?
            .and_then(|row| row.try_get::<String>("", "code").ok())
            .unwrap_or_else(|| wo_id.to_string());
        return Err(crate::errors::org_validation_failed(
            reservation_codes::ISSUE_WO_ENDED,
            format!(
                "Work order {wo_code} is closed or cancelled: no more parts can be issued against its reservation."
            ),
            crate::errors::issue_params(&[("work_order", wo_code), ("article", reservation.article_code.clone())]),
        ));
    }
    let remaining_reserved = reservation.quantity_reserved - reservation.quantity_issued;
    if remaining_reserved + 1e-9 < quantity {
        return Err(AppError::ValidationFailed(vec![format!(
            "Issue exceeds reserved remainder ({remaining_reserved})."
        )]));
    }

    let (on_hand, reserved) = get_balance_snapshot(tx, reservation.article_id, reservation.location_id).await?;
    // What this reservation itself holds (not the location's whole reserved stock, which other
    // reservations make up): `inventory::reservations`.
    let held = crate::inventory::reservations::reservation_held(tx, reservation.id).await?;
    if on_hand + 1e-9 < quantity || held + 1e-9 < quantity {
        return Err(AppError::ValidationFailed(vec![
            "Insufficient on-hand/reserved quantity for issue.".to_string(),
        ]));
    }

    let next_issued = reservation.quantity_issued + quantity;
    let next_status = if next_issued + 1e-9 >= reservation.quantity_reserved {
        "consumed"
    } else {
        "partial"
    };

    upsert_balance(
        tx,
        reservation.article_id,
        reservation.warehouse_id,
        reservation.location_id,
        (on_hand - quantity).max(0.0),
        (reserved - quantity).max(0.0),
    )
    .await?;

    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE stock_reservations
         SET quantity_issued = ?, status = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE id = ?",
        [
            next_issued.into(),
            next_status.to_string().into(),
            reservation.id.into(),
        ],
    ))
    .await?;

    let tx_source_type = source_type
        .map(str::to_string)
        .unwrap_or_else(|| reservation.source_type.clone());
    let issue_id = insert_inventory_transaction(
        tx,
        reservation.article_id,
        reservation.warehouse_id,
        reservation.location_id,
        Some(reservation.id),
        "ISSUE",
        quantity,
        &tx_source_type,
        source_id.or(reservation.source_id),
        source_ref.or(reservation.source_ref.as_deref()),
        None,
        notes,
        None,
    )
    .await?;
    if let Some(lot) = lot_id {
        crate::inventory::lots::set_movement_lot(tx, issue_id, lot).await?;
    }
    set_movement_actor(tx, issue_id, actor_id).await?;
    crate::inventory::reservations::recompute_reserved_in(tx, reservation.article_id, reservation.location_id).await?;
    // Parts issued to a job are valued at the location's weighted-average cost.
    let unit_cost = crate::inventory::valuation::stamp_at_average(
        tx,
        reservation.article_id,
        reservation.location_id,
        quantity,
        issue_id,
    )
    .await?;
    Ok((issue_id, unit_cost))
}

/// Records who made a movement (the ledger insert leaves it empty).
pub(crate) async fn set_movement_actor<C: ConnectionTrait>(
    tx: &C,
    transaction_id: i64,
    actor_id: Option<i64>,
) -> AppResult<()> {
    if let Some(actor) = actor_id {
        tx.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE inventory_transactions SET performed_by_id = ? WHERE id = ?",
            [actor.into(), transaction_id.into()],
        ))
        .await?;
    }
    Ok(())
}

pub async fn return_reserved_stock(
    db: &DatabaseConnection,
    input: InventoryReturnInput,
) -> AppResult<StockReservation> {
    return_reserved_stock_as(db, input, None).await
}

/// Return of issued parts against a reservation, made by `actor_id`.
pub async fn return_reserved_stock_as(
    db: &DatabaseConnection,
    input: InventoryReturnInput,
    actor_id: Option<i64>,
) -> AppResult<StockReservation> {
    if input.quantity <= 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "quantity must be greater than 0.".to_string()
        ]));
    }
    let tx = db.begin().await?;
    let reservation = load_reservation_by_id(&tx, input.reservation_id).await?;
    ensure_reservation_invariants(&reservation)?;
    ensure_active_mutation_context(&tx, reservation.article_id, reservation.location_id).await?;
    // Issued quantity: the row's, or the ledger's when it holds more. When two PCs released the
    // same reservation the row converges on the version sent first, which may not count an issue
    // made here; that issue's movement is still in the ledger (`inventory::reservations`).
    let ledger_issued: f64 = tx
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT COALESCE(SUM(CASE movement_type WHEN 'ISSUE' THEN quantity ELSE -quantity END), 0.0) AS issued
             FROM inventory_transactions WHERE reservation_id = ? AND movement_type IN ('ISSUE', 'RETURN')",
            [reservation.id.into()],
        ))
        .await?
        .map(|row| row.try_get::<f64>("", "issued"))
        .transpose()?
        .unwrap_or(0.0);
    let issued = reservation.quantity_issued.max(ledger_issued);
    if issued + 1e-9 < input.quantity {
        return Err(AppError::ValidationFailed(vec![
            "Return quantity exceeds issued quantity.".to_string(),
        ]));
    }
    // D13: nothing stays reserved for a released reservation or for a work order that is closed
    // or cancelled. Parts returned against it go back to free stock (on hand rises, reserved does
    // not) and the reservation is released; otherwise they are reserved again for the job.
    let ended = crate::inventory::reservations::ended_work_order_of_reservation(&tx, reservation.id).await?;
    let reservation = match ended {
        // Still open on an ended work order (ended before D13, or a reservation this PC made
        // while another closed the work order): its unissued remainder is released first, with
        // the reservation's one release movement, then the return goes to free stock.
        Some((wo_id, end)) if matches!(reservation.status.as_str(), "active" | "partial") => {
            crate::inventory::reservations::release_before_return_after_end(&tx, reservation.id, wo_id, end, actor_id)
                .await?;
            load_reservation_by_id(&tx, reservation.id).await?
        }
        _ => reservation,
    };
    let to_free_stock = reservation.status == "released" || ended.is_some();

    let (on_hand, reserved) = get_balance_snapshot(&tx, reservation.article_id, reservation.location_id).await?;
    // Never above the quantity reserved (the reservation's invariant).
    let next_issued = (issued - input.quantity).clamp(0.0, reservation.quantity_reserved);
    let next_status = if to_free_stock {
        "released"
    } else if next_issued == 0.0 {
        "active"
    } else {
        "partial"
    };

    upsert_balance(
        &tx,
        reservation.article_id,
        reservation.warehouse_id,
        reservation.location_id,
        on_hand + input.quantity,
        if to_free_stock {
            reserved
        } else {
            reserved + input.quantity
        },
    )
    .await?;
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE stock_reservations
         SET quantity_issued = ?, status = ?,
             released_at = CASE WHEN ? = 'released' THEN COALESCE(released_at, strftime('%Y-%m-%dT%H:%M:%SZ','now')) ELSE released_at END,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE id = ?",
        [
            next_issued.into(),
            next_status.to_string().into(),
            next_status.to_string().into(),
            reservation.id.into(),
        ],
    ))
    .await?;
    // Tracked articles go back into the lots they were issued from (latest issue first); others
    // return as one movement.
    let tracking = crate::inventory::lots::article_tracking(&tx, reservation.article_id).await?;
    let allocation: Vec<(Option<i64>, f64)> = if tracking.is_tracked() {
        return_lot_allocation(&tx, reservation.id, input.quantity).await?
    } else {
        vec![(None, input.quantity)]
    };
    for (lot_id, quantity) in allocation {
        let return_id = insert_inventory_transaction(
            &tx,
            reservation.article_id,
            reservation.warehouse_id,
            reservation.location_id,
            Some(reservation.id),
            "RETURN",
            quantity,
            &reservation.source_type,
            reservation.source_id,
            reservation.source_ref.as_deref(),
            None,
            input.notes.as_deref(),
            None,
        )
        .await?;
        if let Some(lot) = lot_id {
            crate::inventory::lots::set_movement_lot(&tx, return_id, lot).await?;
        }
        set_movement_actor(&tx, return_id, actor_id).await?;
        // Returned parts come back at the average they left at (the average is unchanged).
        crate::inventory::valuation::stamp_at_average(
            &tx,
            reservation.article_id,
            reservation.location_id,
            quantity,
            return_id,
        )
        .await?;
    }
    if to_free_stock {
        // A RETURN movement restores on-hand and reserved stock (`inventory::ledger`), also on the
        // PCs that receive it; this RELEASE of the same quantity takes the reserved part back off,
        // so every PC ends with the parts in free stock. It is this return's own release (random
        // sync id), not the one-time release of the reservation.
        let release_id = insert_inventory_transaction(
            &tx,
            reservation.article_id,
            reservation.warehouse_id,
            reservation.location_id,
            Some(reservation.id),
            "RELEASE",
            input.quantity,
            &reservation.source_type,
            reservation.source_id,
            reservation.source_ref.as_deref(),
            None,
            Some("Returned to free stock: the reservation is released or its work order has ended"),
            None,
        )
        .await?;
        set_movement_actor(&tx, release_id, actor_id).await?;
    }
    crate::inventory::reservations::recompute_reserved_in(&tx, reservation.article_id, reservation.location_id).await?;
    tx.commit().await?;
    load_reservation_by_id(db, reservation.id).await
}

/// Lots to return `quantity` into: net issued quantity per lot on this reservation, most recent
/// issue first. Quantity issued before lot tracking (no lot) is returned without a lot.
async fn return_lot_allocation<C: ConnectionTrait>(
    tx: &C,
    reservation_id: i64,
    quantity: f64,
) -> AppResult<Vec<(Option<i64>, f64)>> {
    let rows = tx
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT lot_id,
                    SUM(CASE movement_type WHEN 'ISSUE' THEN quantity WHEN 'RETURN' THEN -quantity ELSE 0 END) AS net,
                    MAX(CASE movement_type WHEN 'ISSUE' THEN id ELSE 0 END) AS last_issue
             FROM inventory_transactions
             WHERE reservation_id = ? AND movement_type IN ('ISSUE', 'RETURN')
             GROUP BY lot_id
             ORDER BY last_issue DESC",
            [reservation_id.into()],
        ))
        .await?;
    let mut remaining = quantity;
    let mut allocation = Vec::new();
    for row in rows {
        if remaining <= 1e-9 {
            break;
        }
        let net: f64 = row.try_get("", "net")?;
        if net <= 1e-9 {
            continue;
        }
        let lot_id: Option<i64> = row.try_get("", "lot_id").ok().flatten();
        let take = net.min(remaining);
        allocation.push((lot_id, take));
        remaining -= take;
    }
    if remaining > 1e-9 {
        allocation.push((None, remaining));
    }
    Ok(allocation)
}

pub(crate) async fn release_stock_reservation_with_connection<C: ConnectionTrait>(
    db: &C,
    reservation_id: i64,
    notes: Option<&str>,
) -> AppResult<(StockReservation, Option<i64>)> {
    let released = release_reservation_in(db, reservation_id, notes, true).await?;
    Ok((released.reservation, released.movement_id))
}

/// Releases a reservation whose need was withdrawn (a cancelled planned work-order part line),
/// also when its article was deactivated since: a release only lowers the reserved quantity, it
/// moves no stock, so the active-article rule of stock movements does not apply. The location
/// must still exist.
pub(crate) async fn release_withdrawn_reservation_with_connection<C: ConnectionTrait>(
    db: &C,
    reservation_id: i64,
    notes: Option<&str>,
) -> AppResult<(StockReservation, Option<i64>)> {
    let released = release_reservation_in(db, reservation_id, notes, false).await?;
    Ok((released.reservation, released.movement_id))
}

/// What releasing one reservation did.
#[derive(Debug, Clone)]
pub(crate) struct ReservationRelease {
    /// The reservation after the call.
    pub reservation: StockReservation,
    /// False when it was already released (nothing was done).
    pub was_open: bool,
    /// Reserved stock freed by this call at the reservation's location: 0 when nothing was left
    /// reserved, or when the release had already arrived from another PC.
    pub quantity_released: f64,
    /// What the reservation row still counted beyond what its movements held (movements from
    /// several PCs drifted apart): released all the same, reported in the audit.
    pub quantity_short: f64,
    /// The RELEASE movement written by this call.
    pub movement_id: Option<i64>,
    /// The RELEASE movement of this reservation had already been received from another PC: it
    /// lowered the reserved stock when it was applied, so only the reservation status is closed.
    pub release_received_from_sync: bool,
}

/// Sync identity of the RELEASE movement of a reservation. A reservation is released once
/// (`released` is terminal), so its release movement has one identity on every PC: two PCs that
/// both release it (both close the work order, or a storekeeper releases it on one PC while the
/// work order is closed on another) write the same movement, and the copy received from the other
/// PC is not applied again (`inventory_transactions` is append-only: a known sync id is a no-op).
/// The reserved stock is a projection of the ledger (`inventory::reservations`): once this movement
/// is present the reservation counts 0 on every PC, whatever quantity each PC released.
pub(crate) async fn reservation_release_movement_sync_id<C: ConnectionTrait>(
    db: &C,
    reservation_id: i64,
) -> AppResult<String> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT entity_sync_id FROM stock_reservations WHERE id = ?",
            [reservation_id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "StockReservation".into(),
            id: reservation_id.to_string(),
        })?;
    let reservation_sync_id = crate::sync::identity::require_entity_sync_id(
        "stock_reservations",
        reservation_id,
        row.try_get("", "entity_sync_id").ok().flatten(),
    )?;
    Ok(crate::sync::identity::seeded_entity_sync_id(
        crate::inventory::reservations::RELEASE_SEED_ENTITY,
        &reservation_sync_id,
    ))
}

pub(crate) async fn release_reservation_in<C: ConnectionTrait>(
    db: &C,
    reservation_id: i64,
    notes: Option<&str>,
    require_active_article: bool,
) -> AppResult<ReservationRelease> {
    let reservation = load_reservation_by_id(db, reservation_id).await?;
    ensure_reservation_invariants(&reservation)?;
    if require_active_article {
        ensure_active_mutation_context(db, reservation.article_id, reservation.location_id).await?;
    } else {
        ensure_location_exists(db, reservation.location_id).await?;
    }
    if reservation.status == "released" {
        return Ok(ReservationRelease {
            reservation,
            was_open: false,
            quantity_released: 0.0,
            quantity_short: 0.0,
            movement_id: None,
            release_received_from_sync: false,
        });
    }
    let remaining_reserved = (reservation.quantity_reserved - reservation.quantity_issued).max(0.0);
    let release_sync_id = reservation_release_movement_sync_id(db, reservation.id).await?;
    // Another PC released this reservation first and its RELEASE movement is already here while
    // the reservation row is not (held as a concurrent edit, or still in the inbox): that movement
    // lowered the reserved stock when it was applied; lowering it again would count it twice.
    let release_received_from_sync = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT 1 AS present FROM inventory_transactions WHERE entity_sync_id = ?",
            [release_sync_id.clone().into()],
        ))
        .await?
        .is_some();

    // What this reservation holds in the reserved-stock projection: the quantity freed. Less than
    // the row's remainder when its movements say less (an issue made on another PC arrived before
    // that PC's row); the difference is reported, never refused (`inventory::reservations`).
    let held = if release_received_from_sync {
        0.0
    } else {
        crate::inventory::reservations::reservation_held(db, reservation.id).await?
    };
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE stock_reservations
         SET status = 'released',
             released_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE id = ?",
        [reservation.id.into()],
    ))
    .await?;
    // The one release movement of the reservation, also for 0: its presence is what makes the
    // reservation count 0 on every PC, whatever else arrives later.
    let movement_id = if release_received_from_sync {
        None
    } else {
        let seeded_id = uuid::Uuid::parse_str(&release_sync_id)
            .map_err(|e| AppError::Internal(anyhow::anyhow!("reservation release sync id: {e}")))?;
        Some(
            insert_inventory_transaction(
                db,
                reservation.article_id,
                reservation.warehouse_id,
                reservation.location_id,
                Some(reservation.id),
                "RELEASE",
                held,
                &reservation.source_type,
                reservation.source_id,
                reservation.source_ref.as_deref(),
                None,
                notes,
                Some(seeded_id),
            )
            .await?,
        )
    };
    crate::inventory::reservations::recompute_reserved_in(db, reservation.article_id, reservation.location_id).await?;
    let quantity_released = held;
    let quantity_short = if release_received_from_sync {
        0.0
    } else {
        (remaining_reserved - held).max(0.0)
    };
    let released = load_reservation_by_id(db, reservation.id).await?;
    Ok(ReservationRelease {
        reservation: released,
        was_open: true,
        quantity_released,
        quantity_short,
        movement_id,
        release_received_from_sync,
    })
}

pub async fn release_stock_reservation(
    db: &DatabaseConnection,
    input: InventoryReleaseReservationInput,
) -> AppResult<StockReservation> {
    let tx = db.begin().await?;
    let (released, _movement_id) =
        release_stock_reservation_with_connection(&tx, input.reservation_id, input.notes.as_deref()).await?;
    tx.commit().await?;
    Ok(released)
}

/// Stable refusal codes of reservations (translated in `inventory:errors`).
pub mod reservation_codes {
    /// An issue against a reservation of a closed or cancelled work order.
    pub const ISSUE_WO_ENDED: &str = "INV_ISSUE_WO_ENDED";
}

/// Stable refusal codes of a store-to-store transfer (translated in `training:errors`).
pub mod transfer_codes {
    pub const QUANTITY_INVALID: &str = "INV_TRANSFER_QUANTITY_INVALID";
    pub const SAME_LOCATION: &str = "INV_TRANSFER_SAME_LOCATION";
    pub const NOTE_TOO_SHORT: &str = "INV_TRANSFER_NOTE_TOO_SHORT";
    pub const ARTICLE_INACTIVE: &str = "INV_TRANSFER_ARTICLE_INACTIVE";
    pub const LOCATION_UNAVAILABLE: &str = "INV_TRANSFER_LOCATION_UNAVAILABLE";
    pub const INSUFFICIENT_AVAILABLE: &str = "INV_TRANSFER_INSUFFICIENT_AVAILABLE";
    pub const WHOLE_UNITS: &str = "INV_TRANSFER_WHOLE_UNITS";
    pub const LOT_UNAVAILABLE: &str = "INV_TRANSFER_LOT_UNAVAILABLE";
}

/// Minimum length (characters, after trimming) of the reason of a transfer.
pub const TRANSFER_NOTE_MIN_CHARS: usize = 5;

fn transfer_refusal(code: &str, message: impl Into<String>, params: &[(&str, String)]) -> AppError {
    crate::errors::org_validation_failed(code, message, crate::errors::issue_params(params))
}

/// A business failure of a check becomes its coded refusal; a real error (database, I/O) is
/// never presented as a business refusal and propagates unchanged.
fn coded_or_propagate(err: AppError, refusal: impl FnOnce() -> AppError) -> AppError {
    match err {
        AppError::ValidationFailed(_) | AppError::NotFound { .. } | AppError::OrgValidationFailed(_) => refusal(),
        other => other,
    }
}

/// Tolerance of the available-stock check: float noise (0.3 - 0.1 = 0.19999999999999998) must not
/// refuse a quantity the screen shows as available.
const TRANSFER_QTY_TOLERANCE: f64 = 1e-9;

/// A quantity as shown to the storekeeper: up to 3 decimals, no trailing zeros.
fn transfer_qty_text(value: f64) -> String {
    let text = format!("{value:.3}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Moves stock between two stores in one transaction. The reserved quantity never moves: only
/// the available stock (on hand minus reserved) can be transferred. The reason is required and
/// is written, with the actor and the time, on the OUT and the IN movement of each lot.
pub async fn transfer_stock(
    db: &DatabaseConnection,
    input: InventoryTransferInput,
    actor_id: Option<i64>,
) -> AppResult<Vec<InventoryStockBalance>> {
    if !input.quantity.is_finite() || input.quantity <= 0.0 {
        return Err(transfer_refusal(
            transfer_codes::QUANTITY_INVALID,
            "quantity must be greater than 0.",
            &[],
        ));
    }
    if input.from_location_id == input.to_location_id {
        return Err(transfer_refusal(
            transfer_codes::SAME_LOCATION,
            "from_location_id and to_location_id must be different.",
            &[],
        ));
    }
    let note = input.notes.as_deref().map(str::trim).unwrap_or_default().to_string();
    if note.chars().count() < TRANSFER_NOTE_MIN_CHARS {
        return Err(transfer_refusal(
            transfer_codes::NOTE_TOO_SHORT,
            format!("A transfer needs a reason of at least {TRANSFER_NOTE_MIN_CHARS} characters."),
            &[("min", TRANSFER_NOTE_MIN_CHARS.to_string())],
        ));
    }

    let tx = db.begin().await?;
    ensure_article_active(&tx, input.article_id).await.map_err(|e| {
        coded_or_propagate(e, || {
            transfer_refusal(
                transfer_codes::ARTICLE_INACTIVE,
                "The article is inactive or unknown.",
                &[],
            )
        })
    })?;
    let from_warehouse = ensure_location_exists(&tx, input.from_location_id).await.map_err(|e| {
        coded_or_propagate(e, || {
            transfer_refusal(
                transfer_codes::LOCATION_UNAVAILABLE,
                "The source store is unknown or inactive.",
                &[("side", "from".to_string())],
            )
        })
    })?;
    let to_warehouse = ensure_location_exists(&tx, input.to_location_id).await.map_err(|e| {
        coded_or_propagate(e, || {
            transfer_refusal(
                transfer_codes::LOCATION_UNAVAILABLE,
                "The destination store is unknown or inactive.",
                &[("side", "to".to_string())],
            )
        })
    })?;

    // Tracked articles move lot by lot (chosen lot, else FEFO) so each lot follows its parts.
    let tracking = crate::inventory::lots::article_tracking(&tx, input.article_id).await?;
    crate::inventory::lots::ensure_whole_units(&tracking, input.quantity).map_err(|e| {
        coded_or_propagate(e, || {
            transfer_refusal(
                transfer_codes::WHOLE_UNITS,
                "Serial-tracked articles move by whole units.",
                &[],
            )
        })
    })?;

    let (from_on_hand, from_reserved) = get_balance_snapshot(&tx, input.article_id, input.from_location_id).await?;
    let quantity_requested = input.quantity;
    let from_available = from_on_hand - from_reserved;
    if from_available + TRANSFER_QTY_TOLERANCE < quantity_requested {
        let available = transfer_qty_text(from_available.max(0.0));
        return Err(transfer_refusal(
            transfer_codes::INSUFFICIENT_AVAILABLE,
            format!(
                "Insufficient available stock at the source store ({available}); reserved stock cannot be transferred."
            ),
            &[
                ("available", available.clone()),
                ("requested", transfer_qty_text(input.quantity)),
            ],
        ));
    }

    let lot_refusal = |err: AppError| {
        let detail = err.to_string();
        coded_or_propagate(err, || {
            transfer_refusal(
                transfer_codes::LOT_UNAVAILABLE,
                detail,
                &[("requested", transfer_qty_text(input.quantity))],
            )
        })
    };
    let allocation: Vec<(Option<i64>, f64)> = if tracking.is_tracked() {
        match input.lot_id {
            Some(lot) => {
                crate::inventory::lots::ensure_lot_available(
                    &tx,
                    lot,
                    input.article_id,
                    input.from_location_id,
                    quantity_requested,
                )
                .await
                .map_err(lot_refusal)?;
                vec![(Some(lot), quantity_requested)]
            }
            None => {
                crate::inventory::lots::fefo_allocate(&tx, input.article_id, input.from_location_id, quantity_requested)
                    .await
                    .map_err(lot_refusal)?
                    .into_iter()
                    .map(|(lot, qty)| (Some(lot), qty))
                    .collect()
            }
        }
    } else {
        vec![(None, quantity_requested)]
    };

    let (to_on_hand, to_reserved) = get_balance_snapshot(&tx, input.article_id, input.to_location_id).await?;

    // Raw arithmetic, like every other movement: the reserved quantity is written back exactly as
    // read (a transfer never changes it). Only when the result falls below the raw reserved
    // quantity by float noise (within the tolerance of the check above) is the on-hand raised to
    // it, so the balance rules never refuse a quantity the check accepted.
    let source_on_hand_after = {
        let after = from_on_hand - quantity_requested;
        if after < from_reserved && from_reserved - after <= TRANSFER_QTY_TOLERANCE {
            from_reserved
        } else {
            after
        }
    };

    upsert_balance(
        &tx,
        input.article_id,
        from_warehouse,
        input.from_location_id,
        source_on_hand_after,
        from_reserved,
    )
    .await?;
    upsert_balance(
        &tx,
        input.article_id,
        to_warehouse,
        input.to_location_id,
        to_on_hand + quantity_requested,
        to_reserved,
    )
    .await?;

    let source_type = input
        .source_type
        .clone()
        .unwrap_or_else(|| "INTERNAL_TRANSFER".to_string());
    let mut destination_on_hand = to_on_hand;
    for (lot_id, quantity) in allocation {
        let out_id = insert_inventory_transaction(
            &tx,
            input.article_id,
            from_warehouse,
            input.from_location_id,
            None,
            "TRANSFER_OUT",
            quantity,
            &source_type,
            input.source_id,
            input.source_ref.as_deref(),
            None,
            Some(note.as_str()),
            None,
        )
        .await?;
        let in_id = insert_inventory_transaction(
            &tx,
            input.article_id,
            to_warehouse,
            input.to_location_id,
            None,
            "TRANSFER_IN",
            quantity,
            &source_type,
            input.source_id,
            input.source_ref.as_deref(),
            None,
            Some(note.as_str()),
            None,
        )
        .await?;
        set_movement_actor(&tx, out_id, actor_id).await?;
        set_movement_actor(&tx, in_id, actor_id).await?;
        if let Some(lot) = lot_id {
            crate::inventory::lots::set_movement_lot(&tx, out_id, lot).await?;
            crate::inventory::lots::set_movement_lot(&tx, in_id, lot).await?;
        }

        // Transferred parts keep their cost: the source average values the move and is blended
        // into the destination's weighted average.
        if let Some(source_avg) = crate::inventory::valuation::stamp_at_average(
            &tx,
            input.article_id,
            input.from_location_id,
            quantity,
            out_id,
        )
        .await?
        {
            crate::inventory::valuation::apply_inbound_cost(
                &tx,
                input.article_id,
                input.to_location_id,
                destination_on_hand,
                quantity,
                source_avg,
                in_id,
                crate::inventory::valuation::COST_SOURCE_MOVING_AVG,
                input.source_ref.as_deref(),
            )
            .await?;
        }
        destination_on_hand += quantity;
    }

    tx.commit().await?;
    let from = load_balance(db, input.article_id, input.from_location_id).await?;
    let to = load_balance(db, input.article_id, input.to_location_id).await?;
    Ok(vec![from, to])
}

pub async fn list_reservations(
    db: &DatabaseConnection,
    filter: StockReservationFilter,
) -> AppResult<Vec<StockReservation>> {
    let mut sql = String::from(
        "SELECT r.id, r.article_id, a.article_code, a.article_name,
                r.warehouse_id, w.code AS warehouse_code, w.name AS warehouse_name,
                r.location_id, sl.code AS location_code, sl.name AS location_name,
                r.source_type, r.source_id, r.source_ref,
                CASE WHEN r.source_type IN ('WORK_ORDER','WORK_ORDER_PART') THEN COALESCE(wo.code, r.source_ref) ELSE NULL END AS work_order_code,
                r.quantity_reserved, r.quantity_issued, r.status, r.notes,
                r.created_by_id, r.created_at, r.updated_at, r.released_at
         FROM stock_reservations r
         JOIN articles a ON a.id = r.article_id
         JOIN warehouses w ON w.id = r.warehouse_id
         JOIN stock_locations sl ON sl.id = r.location_id
         LEFT JOIN work_orders wo ON wo.id = r.source_id AND r.source_type IN ('WORK_ORDER','WORK_ORDER_PART')
         WHERE 1=1",
    );
    let mut values: Vec<sea_orm::Value> = Vec::new();

    if let Some(article_id) = filter.article_id {
        sql.push_str(" AND r.article_id = ?");
        values.push(article_id.into());
    }
    if let Some(warehouse_id) = filter.warehouse_id {
        sql.push_str(" AND r.warehouse_id = ?");
        values.push(warehouse_id.into());
    }
    if let Some(source_type) = filter.source_type {
        sql.push_str(" AND r.source_type = ?");
        values.push(source_type.into());
    }
    if let Some(source_id) = filter.source_id {
        sql.push_str(" AND r.source_id = ?");
        values.push(source_id.into());
    }
    if !filter.include_inactive.unwrap_or(false) {
        sql.push_str(" AND r.status IN ('active','partial')");
    }
    sql.push_str(" ORDER BY r.updated_at DESC");

    let rows = if values.is_empty() {
        db.query_all(Statement::from_string(DbBackend::Sqlite, sql)).await?
    } else {
        db.query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
            .await?
    };

    rows.into_iter()
        .map(|row| {
            Ok(StockReservation {
                id: row.try_get("", "id")?,
                article_id: row.try_get("", "article_id")?,
                article_code: row.try_get("", "article_code")?,
                article_name: row.try_get("", "article_name")?,
                warehouse_id: row.try_get("", "warehouse_id")?,
                warehouse_code: row.try_get("", "warehouse_code")?,
                warehouse_name: row.try_get("", "warehouse_name")?,
                location_id: row.try_get("", "location_id")?,
                location_code: row.try_get("", "location_code")?,
                location_name: row.try_get("", "location_name")?,
                source_type: row.try_get("", "source_type")?,
                source_id: row.try_get("", "source_id")?,
                source_ref: row.try_get("", "source_ref")?,
                work_order_code: row.try_get("", "work_order_code").ok().flatten(),
                quantity_reserved: row.try_get("", "quantity_reserved")?,
                quantity_issued: row.try_get("", "quantity_issued")?,
                status: row.try_get("", "status")?,
                notes: row.try_get("", "notes")?,
                created_by_id: row.try_get("", "created_by_id")?,
                created_at: row.try_get("", "created_at")?,
                updated_at: row.try_get("", "updated_at")?,
                released_at: row.try_get("", "released_at")?,
            })
        })
        .collect()
}

pub async fn list_transactions(
    db: &DatabaseConnection,
    filter: InventoryTransactionFilter,
) -> AppResult<Vec<InventoryTransaction>> {
    let mut sql = String::from(
        "SELECT t.id, t.article_id, a.article_code, a.article_name,
                t.warehouse_id, w.code AS warehouse_code, w.name AS warehouse_name,
                t.location_id, sl.code AS location_code, sl.name AS location_name,
                t.reservation_id, t.movement_type, t.quantity,
                t.source_type, t.source_id, t.source_ref,
                t.reason_code, t.reason, t.performed_by_id, t.performed_at,
                t.lot_id, lot.lot_number, lot.serial_number, t.posting_date, t.unit_cost, t.extended_cost
         FROM inventory_transactions t
         JOIN articles a ON a.id = t.article_id
         JOIN warehouses w ON w.id = t.warehouse_id
         JOIN stock_locations sl ON sl.id = t.location_id
         LEFT JOIN inventory_lots lot ON lot.id = t.lot_id
         WHERE 1=1",
    );
    let mut values: Vec<sea_orm::Value> = Vec::new();

    if let Some(article_id) = filter.article_id {
        sql.push_str(" AND t.article_id = ?");
        values.push(article_id.into());
    }
    if let Some(warehouse_id) = filter.warehouse_id {
        sql.push_str(" AND t.warehouse_id = ?");
        values.push(warehouse_id.into());
    }
    if let Some(source_type) = filter.source_type {
        sql.push_str(" AND t.source_type = ?");
        values.push(source_type.into());
    }
    if let Some(source_id) = filter.source_id {
        sql.push_str(" AND t.source_id = ?");
        values.push(source_id.into());
    }
    sql.push_str(" ORDER BY t.id DESC");
    if let Some(limit) = filter.limit {
        let bounded = if limit <= 0 { 50 } else { limit.min(500) };
        sql.push_str(" LIMIT ?");
        values.push(bounded.into());
    }

    let rows = if values.is_empty() {
        db.query_all(Statement::from_string(DbBackend::Sqlite, sql)).await?
    } else {
        db.query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
            .await?
    };

    rows.into_iter()
        .map(|row| {
            Ok(InventoryTransaction {
                id: row.try_get("", "id")?,
                article_id: row.try_get("", "article_id")?,
                article_code: row.try_get("", "article_code")?,
                article_name: row.try_get("", "article_name")?,
                warehouse_id: row.try_get("", "warehouse_id")?,
                warehouse_code: row.try_get("", "warehouse_code")?,
                warehouse_name: row.try_get("", "warehouse_name")?,
                location_id: row.try_get("", "location_id")?,
                location_code: row.try_get("", "location_code")?,
                location_name: row.try_get("", "location_name")?,
                reservation_id: row.try_get("", "reservation_id")?,
                movement_type: row.try_get("", "movement_type")?,
                quantity: row.try_get("", "quantity")?,
                source_type: row.try_get("", "source_type")?,
                source_id: row.try_get("", "source_id")?,
                source_ref: row.try_get("", "source_ref")?,
                reason_code: row.try_get("", "reason_code")?,
                notes: row.try_get("", "reason")?,
                performed_by_id: row.try_get("", "performed_by_id")?,
                performed_at: row.try_get("", "performed_at")?,
                lot_id: row.try_get("", "lot_id").ok().flatten(),
                lot_number: row.try_get("", "lot_number").ok().flatten(),
                serial_number: row.try_get("", "serial_number").ok().flatten(),
                posting_date: row.try_get("", "posting_date").ok().flatten(),
                unit_cost: row.try_get("", "unit_cost").ok().flatten(),
                extended_cost: row.try_get("", "extended_cost").ok().flatten(),
            })
        })
        .collect()
}

pub async fn evaluate_reorder(
    db: &DatabaseConnection,
    warehouse_id: Option<i64>,
) -> AppResult<Vec<InventoryReorderRecommendation>> {
    let mut sql = String::from(
        "SELECT a.id AS article_id, a.article_code, a.article_name,
                w.id AS warehouse_id, w.code AS warehouse_code,
                a.min_stock, a.reorder_point, a.max_stock,
                COALESCE(SUM(sb.on_hand_qty), 0.0) AS on_hand_qty,
                COALESCE(SUM(sb.reserved_qty), 0.0) AS reserved_qty,
                COALESCE(SUM(sb.available_qty), 0.0) AS available_qty
         FROM articles a
         JOIN warehouses w ON w.is_active = 1
         LEFT JOIN stock_balances sb ON sb.article_id = a.id AND sb.warehouse_id = w.id
         WHERE a.is_active = 1",
    );
    let mut values: Vec<sea_orm::Value> = Vec::new();
    if let Some(warehouse_id) = warehouse_id {
        sql.push_str(" AND w.id = ?");
        values.push(warehouse_id.into());
    }
    sql.push_str(
        " GROUP BY a.id, a.article_code, a.article_name, w.id, w.code,
                  a.min_stock, a.reorder_point, a.max_stock
          ORDER BY a.article_code ASC, w.code ASC, a.id ASC",
    );

    let rows = if values.is_empty() {
        db.query_all(Statement::from_string(DbBackend::Sqlite, sql)).await?
    } else {
        db.query_all(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
            .await?
    };

    let mut recommendations = Vec::new();
    for row in rows {
        let min_stock: f64 = row.try_get("", "min_stock")?;
        let reorder_point: f64 = row.try_get("", "reorder_point")?;
        let max_stock: Option<f64> = row.try_get("", "max_stock")?;
        let available_qty: f64 = row.try_get("", "available_qty")?;

        let trigger_type = if available_qty <= reorder_point && reorder_point > 0.0 {
            Some("reorder_point")
        } else if available_qty < min_stock && min_stock > 0.0 {
            Some("min_stock")
        } else {
            None
        };
        let Some(trigger_type) = trigger_type else {
            continue;
        };

        let target = max_stock.unwrap_or(min_stock.max(reorder_point));
        let suggested = (target - available_qty).max(0.0);
        if suggested <= 0.0 {
            continue;
        }

        recommendations.push(InventoryReorderRecommendation {
            article_id: row.try_get("", "article_id")?,
            article_code: row.try_get("", "article_code")?,
            article_name: row.try_get("", "article_name")?,
            warehouse_id: row.try_get("", "warehouse_id")?,
            warehouse_code: row.try_get("", "warehouse_code")?,
            min_stock,
            reorder_point,
            max_stock,
            on_hand_qty: row.try_get("", "on_hand_qty")?,
            reserved_qty: row.try_get("", "reserved_qty")?,
            available_qty,
            suggested_reorder_qty: suggested,
            trigger_type: trigger_type.to_string(),
        });
    }

    Ok(recommendations)
}

pub async fn evaluate_replenishment(
    db: &DatabaseConnection,
    warehouse_id: Option<i64>,
) -> AppResult<Vec<crate::inventory::domain::InventoryReplenishmentRecommendation>> {
    let base = evaluate_reorder(db, warehouse_id).await?;

    let mut result = Vec::new();
    // The reason is `trigger_type` (worded by the screen); no backend sentence.
    for r in base {
        let transfer_balances = suggest_internal_transfer(db, r.article_id, r.warehouse_id).await?;
        let transfer_options: Vec<crate::inventory::domain::ReplenishmentTransferOption> = transfer_balances
            .into_iter()
            .map(|b| crate::inventory::domain::ReplenishmentTransferOption {
                warehouse_id: b.warehouse_id,
                warehouse_code: b.warehouse_code,
                available_qty: b.available_qty,
            })
            .collect();
        let suggestion_type = if transfer_options.is_empty() {
            "PURCHASE".to_string()
        } else {
            "TRANSFER".to_string()
        };

        // Resolve preferred supplier for this article
        let sup_row = db
            .query_one(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "SELECT sas.supplier_id, s.name AS supplier_name,
                        COALESCE(sas.lead_time_days, s.default_lead_time_days) AS lead_time_days,
                        sas.unit_price_hint
                 FROM inventory_supplier_article_sources sas
                 JOIN inventory_suppliers s ON s.id = sas.supplier_id
                 WHERE sas.article_id = ? AND sas.is_active = 1 AND s.is_active = 1
                   AND s.status_code <> 'BLOCKED'
                 ORDER BY sas.is_preferred DESC, sas.priority ASC
                 LIMIT 1",
                [r.article_id.into()],
            ))
            .await
            .ok()
            .flatten();

        let (suggested_supplier_id, suggested_supplier_name, estimated_cost, expected_arrival) =
            if let Some(row) = sup_row {
                let sid: Option<i64> = row.try_get("", "supplier_id").ok();
                let sname: Option<String> = row.try_get("", "supplier_name").ok().flatten();
                let price: Option<f64> = row.try_get("", "unit_price_hint").ok().flatten();
                let lt_days: Option<i64> = row.try_get("", "lead_time_days").ok().flatten();
                let cost = price.map(|p| p * r.suggested_reorder_qty);
                let arrival = lt_days.map(|d| {
                    let dt = chrono::Utc::now() + chrono::Duration::days(d);
                    dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()
                });
                (sid, sname, cost, arrival)
            } else {
                (None, None, None, None)
            };

        result.push(crate::inventory::domain::InventoryReplenishmentRecommendation {
            article_id: r.article_id,
            article_code: r.article_code,
            article_name: r.article_name,
            warehouse_id: r.warehouse_id,
            warehouse_code: r.warehouse_code,
            min_stock: r.min_stock,
            reorder_point: r.reorder_point,
            max_stock: r.max_stock,
            on_hand_qty: r.on_hand_qty,
            reserved_qty: r.reserved_qty,
            available_qty: r.available_qty,
            suggested_reorder_qty: r.suggested_reorder_qty,
            trigger_type: r.trigger_type,
            suggestion_type,
            suggested_supplier_id,
            suggested_supplier_name,
            estimated_cost,
            expected_arrival,
            transfer_options,
        });
    }
    Ok(result)
}

/// ABC classes by annual consumption value: over the last 12 months, the value of what was
/// consumed (issues to work orders and manual exits to a work order), each movement at its posted
/// cost, or at the article's current average cost when the movement was not valued. Receipts and
/// other exits are not consumption. Articles are ranked by value; the cumulative share sets the
/// class (A up to `procurement.abc_a_pct`, B up to `procurement.abc_b_pct`, else C). Active
/// articles with no valued consumption are C. Returns the number of articles classified (0 and
/// no change when nothing valued was consumed).
pub async fn calculate_abc_classification(db: &DatabaseConnection) -> AppResult<i64> {
    let a_pct: f64 = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT CAST(setting_value_json AS REAL) AS v FROM app_settings WHERE setting_key = 'procurement.abc_a_pct' LIMIT 1".to_string(),
        ))
        .await?
        .and_then(|r| r.try_get::<Option<f64>>("", "v").ok().flatten())
        .unwrap_or(80.0);
    let b_pct: f64 = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT CAST(setting_value_json AS REAL) AS v FROM app_settings WHERE setting_key = 'procurement.abc_b_pct' LIMIT 1".to_string(),
        ))
        .await?
        .and_then(|r| r.try_get::<Option<f64>>("", "v").ok().flatten())
        .unwrap_or(95.0);

    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT t.article_id,
                    SUM(COALESCE(t.extended_cost,
                                 t.quantity * (SELECT SUM(sb.on_hand_qty * sb.moving_avg_unit_cost)
                                                      / NULLIF(SUM(sb.on_hand_qty), 0)
                                               FROM stock_balances sb
                                               WHERE sb.article_id = t.article_id
                                                 AND sb.moving_avg_unit_cost IS NOT NULL))) AS consumption_value
             FROM inventory_transactions t
             WHERE (t.movement_type = 'ISSUE'
                    OR (t.movement_type = 'ADJUST_OUT' AND t.reason_code = 'SORTIE_OT'))
               AND COALESCE(t.posting_date, substr(t.performed_at, 1, 10))
                   >= date('now', '-12 months')
             GROUP BY t.article_id
             HAVING consumption_value > 0
             ORDER BY consumption_value DESC"
                .to_string(),
        ))
        .await?;

    let ranked: Vec<(i64, f64)> = rows
        .iter()
        .map(|r| {
            Ok((
                r.try_get::<i64>("", "article_id")?,
                r.try_get::<f64>("", "consumption_value")?,
            ))
        })
        .collect::<AppResult<Vec<_>>>()?;
    let total: f64 = ranked.iter().map(|(_, v)| v).sum();
    if total <= 0.0 {
        return Ok(0);
    }

    let tx = db.begin().await?;
    let mut cumulative = 0.0;
    let mut updated_count = 0i64;
    for (article_id, value) in &ranked {
        // The class is decided by the share *before* this article, so the article that crosses
        // the A threshold is still an A item.
        let share_before = cumulative / total * 100.0;
        cumulative += value;
        let class = if share_before < a_pct {
            "A"
        } else if share_before < b_pct {
            "B"
        } else {
            "C"
        };
        tx.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE articles SET abc_class_code = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?",
            [class.into(), (*article_id).into()],
        ))
        .await?;
        updated_count += 1;
    }
    let ranked_ids: Vec<String> = ranked.iter().map(|(id, _)| id.to_string()).collect();
    let others = tx
        .execute(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "UPDATE articles SET abc_class_code = 'C', updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                 WHERE is_active = 1 AND id NOT IN ({}) AND COALESCE(abc_class_code, '') <> 'C'",
                ranked_ids.join(",")
            ),
        ))
        .await?;
    tx.commit().await?;
    Ok(updated_count + others.rows_affected() as i64)
}

pub async fn calculate_xyz_classification(db: &DatabaseConnection) -> AppResult<i64> {
    let x_cv_max: f64 = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT CAST(setting_value_json AS REAL) AS v FROM app_settings WHERE setting_key = 'procurement.xyz_x_cv_max' LIMIT 1".to_string(),
        ))
        .await?
        .and_then(|r| r.try_get::<Option<f64>>("", "v").ok().flatten())
        .unwrap_or(0.5);
    let y_cv_max: f64 = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT CAST(setting_value_json AS REAL) AS v FROM app_settings WHERE setting_key = 'procurement.xyz_y_cv_max' LIMIT 1".to_string(),
        ))
        .await?
        .and_then(|r| r.try_get::<Option<f64>>("", "v").ok().flatten())
        .unwrap_or(1.0);

    // Monthly demand variability (CV = stddev/mean)
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT t.article_id,
                    AVG(monthly_qty) AS mean_qty,
                    -- SQLite doesn't have stddev, approximate with variance manually
                    COUNT(*) AS period_count,
                    SUM(monthly_qty * monthly_qty) AS sum_sq,
                    SUM(monthly_qty) AS sum_qty
             FROM (
               SELECT article_id,
                      strftime('%Y-%m', performed_at) AS month,
                      SUM(quantity) AS monthly_qty
               FROM inventory_transactions
               WHERE movement_type IN ('ISSUE','ADJUST_OUT')
                 AND performed_at >= strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-12 months')
               GROUP BY article_id, strftime('%Y-%m', performed_at)
             ) t
             GROUP BY t.article_id
             HAVING COUNT(*) >= 2"
                .to_string(),
        ))
        .await
        .unwrap_or_default();

    let mut updated_count = 0i64;
    for row in &rows {
        let article_id: i64 = row.try_get("", "article_id").unwrap_or(0);
        let n: f64 = row.try_get::<i64>("", "period_count").unwrap_or(1) as f64;
        let sum_sq: f64 = row.try_get("", "sum_sq").unwrap_or(0.0);
        let sum_qty: f64 = row.try_get("", "sum_qty").unwrap_or(0.0);
        let mean = sum_qty / n;
        let variance = if n > 1.0 { (sum_sq / n) - (mean * mean) } else { 0.0 };
        let stddev = variance.max(0.0).sqrt();
        let cv = if mean > 0.0 { stddev / mean } else { 0.0 };
        let class = if cv <= x_cv_max {
            "X"
        } else if cv <= y_cv_max {
            "Y"
        } else {
            "Z"
        };
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE articles SET xyz_class_code = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?",
            [class.into(), article_id.into()],
        ))
        .await
        .ok();
        updated_count += 1;
    }
    Ok(updated_count)
}

pub async fn list_article_equivalents(
    db: &DatabaseConnection,
    article_id: i64,
) -> AppResult<Vec<crate::inventory::domain::ArticleEquivalent>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT ae.id, ae.article_id, a1.article_code, a1.article_name,
                    ae.equivalent_article_id, a2.article_code AS equivalent_code, a2.article_name AS equivalent_name,
                    ae.equivalence_type, ae.notes, ae.is_bidirectional, ae.created_at
             FROM inventory_article_equivalents ae
             JOIN articles a1 ON a1.id = ae.article_id
             JOIN articles a2 ON a2.id = ae.equivalent_article_id
             WHERE ae.article_id = ? OR (ae.equivalent_article_id = ? AND ae.is_bidirectional = 1)
             ORDER BY ae.equivalence_type ASC, a2.article_code ASC",
            [article_id.into(), article_id.into()],
        ))
        .await?;
    rows.into_iter()
        .map(|row| {
            Ok(crate::inventory::domain::ArticleEquivalent {
                id: row.try_get("", "id")?,
                article_id: row.try_get("", "article_id")?,
                article_code: row.try_get("", "article_code")?,
                article_name: row.try_get("", "article_name")?,
                equivalent_article_id: row.try_get("", "equivalent_article_id")?,
                equivalent_code: row.try_get("", "equivalent_code")?,
                equivalent_name: row.try_get("", "equivalent_name")?,
                equivalence_type: row.try_get("", "equivalence_type")?,
                notes: row.try_get("", "notes")?,
                is_bidirectional: row.try_get("", "is_bidirectional")?,
                created_at: row.try_get("", "created_at")?,
            })
        })
        .collect()
}

pub async fn upsert_article_equivalent(
    db: &DatabaseConnection,
    input: crate::inventory::domain::ArticleEquivalentInput,
) -> AppResult<crate::inventory::domain::ArticleEquivalent> {
    if input.article_id == input.equivalent_article_id {
        return Err(AppError::ValidationFailed(vec![
            "An article cannot be equivalent to itself.".to_string(),
        ]));
    }
    let allowed = ["DIRECT", "FUNCTIONAL", "UPGRADE"];
    if !allowed.contains(&input.equivalence_type.as_str()) {
        return Err(AppError::ValidationFailed(vec![
            "equivalence_type must be one of: DIRECT, FUNCTIONAL, UPGRADE.".to_string(),
        ]));
    }
    let is_bidirectional = input.is_bidirectional.unwrap_or(true) as i64;
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inventory_article_equivalents
            (article_id, equivalent_article_id, equivalence_type, notes, is_bidirectional, created_at)
         VALUES (?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now'))
         ON CONFLICT(article_id, equivalent_article_id) DO UPDATE SET
            equivalence_type = excluded.equivalence_type,
            notes = excluded.notes,
            is_bidirectional = excluded.is_bidirectional",
        [
            input.article_id.into(),
            input.equivalent_article_id.into(),
            input.equivalence_type.into(),
            input.notes.map_or(Value::String(None), Value::from),
            is_bidirectional.into(),
        ],
    ))
    .await?;

    let equivalents = list_article_equivalents(db, input.article_id).await?;
    equivalents
        .into_iter()
        .find(|e| e.equivalent_article_id == input.equivalent_article_id)
        .ok_or_else(|| AppError::ValidationFailed(vec!["Failed to retrieve equivalent.".to_string()]))
}

pub async fn delete_article_equivalent(db: &DatabaseConnection, equivalent_id: i64) -> AppResult<()> {
    let exists = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM inventory_article_equivalents WHERE id = ?",
            [equivalent_id.into()],
        ))
        .await?;
    if exists.is_none() {
        return Err(AppError::NotFound {
            entity: "inventory_article_equivalents".to_string(),
            id: equivalent_id.to_string(),
        });
    }
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "DELETE FROM inventory_article_equivalents WHERE id = ?",
        [equivalent_id.into()],
    ))
    .await?;
    Ok(())
}

pub async fn list_article_purchase_history(
    db: &DatabaseConnection,
    article_id: i64,
) -> AppResult<Vec<crate::inventory::domain::ArticlePurchaseHistoryRow>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT po.id AS purchase_order_id, po.po_number, po.ordered_at,
                    po.supplier_id, s.name AS supplier_name,
                    pol.unit_price, pol.ordered_qty, pol.received_qty, pol.status
             FROM purchase_order_lines pol
             JOIN purchase_orders po ON po.id = pol.purchase_order_id
             LEFT JOIN inventory_suppliers s ON s.id = po.supplier_id
             WHERE pol.article_id = ?
             ORDER BY po.ordered_at DESC NULLS LAST, po.id DESC",
            [article_id.into()],
        ))
        .await?;
    rows.into_iter()
        .map(|row| {
            Ok(crate::inventory::domain::ArticlePurchaseHistoryRow {
                purchase_order_id: row.try_get("", "purchase_order_id")?,
                po_number: row.try_get("", "po_number")?,
                ordered_at: row.try_get("", "ordered_at").ok().flatten(),
                supplier_id: row.try_get("", "supplier_id").ok().flatten(),
                supplier_name: row.try_get("", "supplier_name").ok().flatten(),
                unit_price: row.try_get("", "unit_price").ok().flatten(),
                ordered_qty: row.try_get("", "ordered_qty")?,
                received_qty: row.try_get("", "received_qty")?,
                status: row.try_get("", "status")?,
            })
        })
        .collect()
}

/// Requisition statuses counted as open by the dashboard (and opened by its tile).
pub const OPEN_REQUISITION_STATUSES: &[&str] = &["DRAFT", "SUBMITTED", "REJECTED"];
/// Purchase-order statuses counted as open (submitted ones have their own "to approve" tile).
pub const OPEN_PO_STATUSES: &[&str] = &["DRAFT", "APPROVED", "PARTIALLY_RECEIVED"];
/// Purchase-order statuses that can be late (expected delivery before now).
pub const LATE_PO_STATUSES: &[&str] = &["APPROVED", "PARTIALLY_RECEIVED"];

fn sql_in(values: &[&str]) -> String {
    values.iter().map(|v| format!("'{v}'")).collect::<Vec<_>>().join(",")
}

/// Ids of the active critical spares (`articles.is_critical_spare`).
async fn critical_spare_ids(db: &DatabaseConnection) -> AppResult<std::collections::HashSet<i64>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM articles WHERE is_active = 1 AND COALESCE(is_critical_spare, 0) = 1".to_string(),
        ))
        .await?;
    rows.iter()
        .map(|r| r.try_get::<i64>("", "id").map_err(AppError::from))
        .collect()
}

pub async fn get_procurement_dashboard_summary(
    db: &DatabaseConnection,
) -> AppResult<crate::inventory::domain::ProcurementDashboardSummary> {
    let open_req = sql_in(OPEN_REQUISITION_STATUSES);
    let open_po = sql_in(OPEN_PO_STATUSES);
    let late_po = sql_in(LATE_PO_STATUSES);
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT
                (SELECT COUNT(*) FROM procurement_requisitions WHERE status IN ({open_req})) AS open_requisitions,
                (SELECT COUNT(*) FROM purchase_orders WHERE status = 'SUBMITTED') AS pending_approval_pos,
                (SELECT COUNT(*) FROM purchase_orders WHERE status IN ({open_po})) AS open_pos,
                (SELECT COUNT(*) FROM purchase_orders
                 WHERE status IN ({late_po})
                   AND expected_delivery_date IS NOT NULL
                   AND expected_delivery_date < strftime('%Y-%m-%dT%H:%M:%SZ','now')) AS overdue_pos,
                (SELECT COUNT(*) FROM purchase_orders WHERE status IN ('APPROVED','PARTIALLY_RECEIVED')) AS pending_receipts,
                (SELECT COUNT(*) FROM repairable_orders
                 WHERE status IN ('REQUESTED','RELEASED','SENT_FOR_REPAIR','RETURNED_FROM_REPAIR')) AS repairables_in_repair,
                (SELECT COUNT(*) FROM inventory_suppliers
                 WHERE is_active = 1 AND status_code <> 'BLOCKED') AS active_suppliers_count,
                (SELECT COUNT(*) FROM purchase_orders
                 WHERE status IN ('APPROVED','PARTIALLY_RECEIVED')
                   AND expected_delivery_date IS NOT NULL
                   AND date(expected_delivery_date) = date('now')) AS receiving_today_count"
            ),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("dashboard summary query failed")))?;
    // Low stock: the one definition (`evaluate_reorder`), counted per article over every store.
    let triggers = reorder_triggers_by_article(db).await?;
    let critical = critical_spare_ids(db).await?;
    let low_stock_articles = i64::try_from(triggers.len()).unwrap_or(i64::MAX);
    let critical_low_stock_articles =
        i64::try_from(triggers.keys().filter(|id| critical.contains(id)).count()).unwrap_or(i64::MAX);
    let to_vec = |v: &[&str]| v.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
    Ok(crate::inventory::domain::ProcurementDashboardSummary {
        open_requisitions: row.try_get("", "open_requisitions")?,
        pending_approval_pos: row.try_get("", "pending_approval_pos")?,
        open_pos: row.try_get("", "open_pos")?,
        overdue_pos: row.try_get("", "overdue_pos")?,
        pending_receipts: row.try_get("", "pending_receipts")?,
        low_stock_articles,
        critical_low_stock_articles,
        repairables_in_repair: row.try_get("", "repairables_in_repair")?,
        active_suppliers_count: row.try_get("", "active_suppliers_count")?,
        receiving_today_count: row.try_get("", "receiving_today_count")?,
        open_requisition_statuses: to_vec(OPEN_REQUISITION_STATUSES),
        open_po_statuses: to_vec(OPEN_PO_STATUSES),
        late_po_statuses: to_vec(LATE_PO_STATUSES),
    })
}

fn alert_params(pairs: &[(&str, String)]) -> std::collections::BTreeMap<String, String> {
    pairs.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect()
}

/// Operational alerts for the procurement dashboard, top 3 per kind. Coded (kind + params): the
/// screen words them in the UI language. A failed read fails the call (never "no alert").
pub async fn get_procurement_alerts(
    db: &DatabaseConnection,
) -> AppResult<Vec<crate::inventory::domain::ProcurementAlert>> {
    use crate::inventory::domain::ProcurementAlert;
    let mut alerts = Vec::new();

    // Critical spares that need replenishing — the one low-stock definition (`evaluate_reorder`),
    // lowest availability first.
    let critical = critical_spare_ids(db).await?;
    let mut critical_recs: Vec<_> = evaluate_reorder(db, None)
        .await?
        .into_iter()
        .filter(|r| critical.contains(&r.article_id))
        .collect();
    critical_recs.sort_by(|a, b| a.available_qty.total_cmp(&b.available_qty));
    let mut seen = std::collections::HashSet::new();
    for r in critical_recs.into_iter().filter(|r| seen.insert(r.article_id)).take(3) {
        let threshold = if r.trigger_type == "reorder_point" {
            r.reorder_point
        } else {
            r.min_stock
        };
        alerts.push(ProcurementAlert {
            kind: "CRITICAL_STOCK".to_string(),
            severity: "high".to_string(),
            entity_type: Some("ARTICLE".to_string()),
            entity_id: Some(r.article_id),
            entity_code: Some(r.article_code.clone()),
            params: alert_params(&[
                ("name", r.article_name.clone()),
                ("warehouse_code", r.warehouse_code.clone()),
                ("available_qty", r.available_qty.to_string()),
                ("threshold_qty", threshold.to_string()),
                ("trigger", r.trigger_type.clone()),
            ]),
        });
    }

    let late_rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            format!(
                "SELECT id, po_number, expected_delivery_date
                 FROM purchase_orders
                 WHERE status IN ({})
                   AND expected_delivery_date IS NOT NULL
                   AND expected_delivery_date < strftime('%Y-%m-%dT%H:%M:%SZ','now')
                 ORDER BY expected_delivery_date ASC
                 LIMIT 3",
                sql_in(LATE_PO_STATUSES)
            ),
        ))
        .await?;
    for row in late_rows {
        let po_number: String = row.try_get("", "po_number")?;
        let eta: String = row.try_get("", "expected_delivery_date")?;
        alerts.push(ProcurementAlert {
            kind: "LATE_PO".to_string(),
            severity: "high".to_string(),
            entity_type: Some("PURCHASE_ORDER".to_string()),
            entity_id: Some(row.try_get("", "id")?),
            entity_code: Some(po_number),
            params: alert_params(&[("expected_date", eta)]),
        });
    }

    // Only orders whose repair lead time is known (supplier, else article): no assumed delay.
    let repair_rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT ro.id, ro.order_code, ro.sent_at, a.article_code,
                    COALESCE(s.default_lead_time_days, a.lead_time_days) AS lead_sla_days
             FROM repairable_orders ro
             JOIN articles a ON a.id = ro.article_id
             LEFT JOIN inventory_suppliers s ON s.id = ro.vendor_supplier_id
             WHERE ro.status = 'SENT_FOR_REPAIR'
               AND ro.sent_at IS NOT NULL
               AND COALESCE(s.default_lead_time_days, a.lead_time_days) IS NOT NULL
               AND datetime(ro.sent_at, '+' || COALESCE(s.default_lead_time_days, a.lead_time_days) || ' days')
                   < strftime('%Y-%m-%dT%H:%M:%SZ','now')
             ORDER BY ro.sent_at ASC
             LIMIT 3"
                .to_string(),
        ))
        .await?;
    for row in repair_rows {
        let code: String = row.try_get("", "order_code")?;
        let article: String = row.try_get("", "article_code")?;
        let sla: i64 = row.try_get("", "lead_sla_days")?;
        alerts.push(ProcurementAlert {
            kind: "REPAIRABLE_OVERDUE".to_string(),
            severity: "medium".to_string(),
            entity_type: Some("REPAIRABLE_ORDER".to_string()),
            entity_id: Some(row.try_get("", "id")?),
            entity_code: Some(code),
            params: alert_params(&[("article_code", article), ("lead_days", sla.to_string())]),
        });
    }

    let supplier_rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT s.id, s.code, s.name, s.default_lead_time_days,
                    (SELECT COUNT(*) FROM goods_receipt_lines grl
                     JOIN purchase_order_lines pol ON pol.id = grl.po_line_id
                     JOIN purchase_orders po ON po.id = pol.purchase_order_id
                     WHERE po.supplier_id = s.id) AS total_lines,
                    (SELECT SUM(CASE
                          WHEN po.expected_delivery_date IS NOT NULL
                           AND date(COALESCE(gr.received_at, grl.created_at)) <= date(po.expected_delivery_date)
                           AND grl.accepted_qty >= COALESCE(grl.ordered_qty, pol.ordered_qty)
                          THEN 1
                          WHEN po.expected_delivery_date IS NULL
                           AND grl.accepted_qty >= COALESCE(grl.ordered_qty, pol.ordered_qty)
                          THEN 1 ELSE 0 END)
                     FROM goods_receipt_lines grl
                     JOIN purchase_order_lines pol ON pol.id = grl.po_line_id
                     JOIN purchase_orders po ON po.id = pol.purchase_order_id
                     LEFT JOIN goods_receipts gr ON gr.id = grl.goods_receipt_id
                     WHERE po.supplier_id = s.id) AS on_time_lines,
                    (SELECT AVG(grl.actual_lead_time_days)
                     FROM goods_receipt_lines grl
                     JOIN purchase_order_lines pol ON pol.id = grl.po_line_id
                     JOIN purchase_orders po ON po.id = pol.purchase_order_id
                     WHERE po.supplier_id = s.id) AS avg_lead_time,
                    (SELECT SUM(COALESCE(grl.accepted_qty, 0.0) + COALESCE(grl.rejected_qty, 0.0))
                     FROM goods_receipt_lines grl
                     JOIN purchase_order_lines pol ON pol.id = grl.po_line_id
                     JOIN purchase_orders po ON po.id = pol.purchase_order_id
                     WHERE po.supplier_id = s.id) AS total_received,
                    (SELECT SUM(COALESCE(grl.rejected_qty, 0.0))
                     FROM goods_receipt_lines grl
                     JOIN purchase_order_lines pol ON pol.id = grl.po_line_id
                     JOIN purchase_orders po ON po.id = pol.purchase_order_id
                     WHERE po.supplier_id = s.id) AS total_rejected
             FROM inventory_suppliers s
             WHERE s.is_active = 1 AND s.status_code <> 'BLOCKED'"
                .to_string(),
        ))
        .await?;

    let mut high_risk = Vec::new();
    for row in supplier_rows {
        let total: i64 = row.try_get::<Option<i64>>("", "total_lines")?.unwrap_or(0);
        if total <= 0 {
            continue;
        }
        let on_time: i64 = row.try_get::<Option<i64>>("", "on_time_lines")?.unwrap_or(0);
        let avg_lt: Option<f64> = row.try_get("", "avg_lead_time").ok().flatten();
        let total_recv: f64 = row.try_get::<Option<f64>>("", "total_received")?.unwrap_or(0.0);
        let total_rej: f64 = row.try_get::<Option<f64>>("", "total_rejected")?.unwrap_or(0.0);
        let otif = Some(on_time as f64 / total as f64 * 100.0);
        let rejected = if total_recv > 0.0 {
            Some(total_rej / total_recv * 100.0)
        } else {
            None
        };
        let promised: Option<i64> = row.try_get("", "default_lead_time_days").ok().flatten();
        let risk = crate::inventory::suppliers::compute_supplier_risk_level(otif, rejected, avg_lt, promised);
        if risk.as_deref() == Some("HIGH") {
            let code: String = row.try_get("", "code")?;
            let name: String = row.try_get("", "name")?;
            high_risk.push(ProcurementAlert {
                kind: "SUPPLIER_DELAY".to_string(),
                severity: "medium".to_string(),
                entity_type: Some("SUPPLIER".to_string()),
                entity_id: Some(row.try_get("", "id")?),
                entity_code: Some(code),
                params: alert_params(&[("name", name)]),
            });
        }
    }
    high_risk.truncate(3);
    alerts.extend(high_risk);

    Ok(alerts)
}

pub async fn get_article_consumption_monthly(
    db: &DatabaseConnection,
    article_id: i64,
    months: Option<i64>,
) -> AppResult<Vec<crate::inventory::domain::ArticleConsumptionMonth>> {
    let months = months.unwrap_or(12).clamp(1, 36);
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT strftime('%Y-%m', t.performed_at) AS year_month,
                    COALESCE(SUM(ABS(t.quantity)), 0) AS issued_qty
             FROM inventory_transactions t
             WHERE t.article_id = ?
               AND t.movement_type IN ('ISSUE','ADJUST_OUT')
               AND t.performed_at >= strftime('%Y-%m-%dT%H:%M:%SZ', 'now', ?)
             GROUP BY year_month
             ORDER BY year_month ASC",
            [article_id.into(), format!("-{} months", months).into()],
        ))
        .await?;
    rows.into_iter()
        .map(|row| {
            Ok(crate::inventory::domain::ArticleConsumptionMonth {
                year_month: row.try_get("", "year_month")?,
                issued_qty: row.try_get("", "issued_qty")?,
            })
        })
        .collect()
}

pub async fn get_article_repairable_history(
    db: &DatabaseConnection,
    article_id: i64,
) -> AppResult<Vec<crate::inventory::domain::ArticleRepairableHistory>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, order_code, quantity, status, reason,
                    COALESCE(repair_cost, NULL) AS repair_cost, created_at, updated_at
             FROM repairable_orders
             WHERE article_id = ?
             ORDER BY created_at DESC",
            [article_id.into()],
        ))
        .await?;
    rows.into_iter()
        .map(|row| {
            Ok(crate::inventory::domain::ArticleRepairableHistory {
                order_id: row.try_get("", "id")?,
                order_code: row.try_get("", "order_code")?,
                quantity: row.try_get("", "quantity")?,
                status: row.try_get("", "status")?,
                reason: row.try_get("", "reason").ok().flatten(),
                repair_cost: row.try_get("", "repair_cost").ok().flatten(),
                created_at: row.try_get("", "created_at")?,
                updated_at: row.try_get("", "updated_at")?,
            })
        })
        .collect()
}

pub async fn list_inventory_document_links(
    db: &DatabaseConnection,
    entity_type: &str,
    entity_id: i64,
) -> AppResult<Vec<crate::inventory::domain::InventoryDocumentLink>> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_type, entity_id, document_ref, link_purpose, is_primary,
                    valid_from, valid_to, created_by_id, created_at
             FROM inventory_document_links
             WHERE entity_type = ? AND entity_id = ?
             ORDER BY is_primary DESC, created_at DESC",
            [entity_type.into(), entity_id.into()],
        ))
        .await?;
    rows.into_iter()
        .map(|row| {
            Ok(crate::inventory::domain::InventoryDocumentLink {
                id: row.try_get("", "id")?,
                entity_type: row.try_get("", "entity_type")?,
                entity_id: row.try_get("", "entity_id")?,
                document_ref: row.try_get("", "document_ref")?,
                link_purpose: row.try_get("", "link_purpose")?,
                is_primary: row.try_get("", "is_primary")?,
                valid_from: row.try_get("", "valid_from")?,
                valid_to: row.try_get("", "valid_to").ok().flatten(),
                created_by_id: row.try_get("", "created_by_id").ok().flatten(),
                created_at: row.try_get("", "created_at")?,
            })
        })
        .collect()
}

pub async fn upsert_inventory_document_link(
    db: &DatabaseConnection,
    input: crate::inventory::domain::InventoryDocumentLinkInput,
) -> AppResult<crate::inventory::domain::InventoryDocumentLink> {
    const ALLOWED_ENTITY_TYPES: &[&str] = &["SUPPLIER", "PURCHASE_ORDER", "REPAIRABLE_ORDER", "ARTICLE"];
    let entity_type = input.entity_type.trim().to_uppercase();
    if !ALLOWED_ENTITY_TYPES.contains(&entity_type.as_str()) {
        return Err(AppError::ValidationFailed(vec![format!(
            "Unsupported document entity_type '{entity_type}'. Allowed: SUPPLIER, PURCHASE_ORDER, REPAIRABLE_ORDER, ARTICLE."
        )]));
    }
    if input.document_ref.trim().is_empty() {
        return Err(AppError::ValidationFailed(
            vec!["document_ref is required.".to_string()],
        ));
    }
    if input.link_purpose.trim().is_empty() {
        return Err(AppError::ValidationFailed(
            vec!["link_purpose is required.".to_string()],
        ));
    }
    let is_primary = input.is_primary.unwrap_or(false) as i64;
    let valid_from = input
        .valid_from
        .clone()
        .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string());
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inventory_document_links
            (entity_type, entity_id, document_ref, link_purpose, is_primary, valid_from, valid_to, created_by_id, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        [
            entity_type.into(),
            input.entity_id.into(),
            input.document_ref.clone().into(),
            input.link_purpose.clone().into(),
            is_primary.into(),
            valid_from.into(),
            input.valid_to.clone().map_or(Value::String(None), Value::from),
            input.created_by_id.map_or(Value::BigInt(None), Value::from),
        ],
    ))
    .await?;
    let id: i64 = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT last_insert_rowid() AS id".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::ValidationFailed(vec!["Failed to create document link.".to_string()]))?
        .try_get("", "id")?;
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, entity_type, entity_id, document_ref, link_purpose, is_primary,
                    valid_from, valid_to, created_by_id, created_at
             FROM inventory_document_links WHERE id = ?",
            [id.into()],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "inventory_document_links".to_string(),
            id: id.to_string(),
        })?;
    Ok(crate::inventory::domain::InventoryDocumentLink {
        id: row.try_get("", "id")?,
        entity_type: row.try_get("", "entity_type")?,
        entity_id: row.try_get("", "entity_id")?,
        document_ref: row.try_get("", "document_ref")?,
        link_purpose: row.try_get("", "link_purpose")?,
        is_primary: row.try_get("", "is_primary")?,
        valid_from: row.try_get("", "valid_from")?,
        valid_to: row.try_get("", "valid_to").ok().flatten(),
        created_by_id: row.try_get("", "created_by_id").ok().flatten(),
        created_at: row.try_get("", "created_at")?,
    })
}

/// Projects the stock position of an article after a hypothetical quantity change,
/// optionally scoped to one warehouse and optionally counting quantity still due on open POs.
///
/// Open PO quantity is attributed to a warehouse through the requisition line's preferred
/// location, the only destination known before goods receipt; lines with no declared
/// destination are therefore only counted in the company-wide (unscoped) projection.
pub async fn project_stock_impact(
    db: &DatabaseConnection,
    article_id: i64,
    warehouse_id: Option<i64>,
    delta_qty: f64,
    include_open_po_qty: bool,
) -> AppResult<StockImpactProjection> {
    if !delta_qty.is_finite() {
        return Err(AppError::ValidationFailed(vec![
            "delta_qty must be a finite number.".to_string()
        ]));
    }

    let mut balance_sql = String::from(
        "SELECT a.article_code, a.article_name,
                COALESCE(SUM(sb.on_hand_qty), 0.0) AS on_hand_qty,
                COALESCE(SUM(sb.reserved_qty), 0.0) AS reserved_qty,
                COALESCE(SUM(sb.available_qty), 0.0) AS available_qty
         FROM articles a
         LEFT JOIN stock_balances sb ON sb.article_id = a.id",
    );
    let mut balance_values: Vec<Value> = Vec::new();
    if let Some(warehouse_id) = warehouse_id {
        balance_sql.push_str(" AND sb.warehouse_id = ?");
        balance_values.push(warehouse_id.into());
    }
    balance_sql.push_str(" WHERE a.id = ? GROUP BY a.id");
    balance_values.push(article_id.into());

    let balance_row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            balance_sql,
            balance_values,
        ))
        .await?
        .ok_or_else(|| AppError::NotFound {
            entity: "articles".to_string(),
            id: article_id.to_string(),
        })?;

    let warehouse_code: Option<String> = match warehouse_id {
        Some(warehouse_id) => {
            let row = db
                .query_one(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "SELECT code FROM warehouses WHERE id = ?",
                    [warehouse_id.into()],
                ))
                .await?
                .ok_or_else(|| AppError::NotFound {
                    entity: "warehouses".to_string(),
                    id: warehouse_id.to_string(),
                })?;
            Some(row.try_get("", "code")?)
        }
        None => None,
    };

    let incoming_open_po_qty = if include_open_po_qty {
        let mut sql = String::from(
            "SELECT COALESCE(SUM(pol.ordered_qty - pol.received_qty), 0.0) AS incoming_qty
             FROM purchase_order_lines pol
             JOIN purchase_orders po ON po.id = pol.purchase_order_id
             LEFT JOIN procurement_requisition_lines rl ON rl.id = pol.requisition_line_id
             LEFT JOIN stock_locations dest ON dest.id = rl.preferred_location_id
             WHERE pol.article_id = ?
               AND pol.status = 'OPEN'
               AND pol.ordered_qty > pol.received_qty
               AND po.status IN ('DRAFT', 'SUBMITTED', 'APPROVED', 'PARTIALLY_RECEIVED')",
        );
        let mut values: Vec<Value> = vec![article_id.into()];
        if let Some(warehouse_id) = warehouse_id {
            sql.push_str(" AND dest.warehouse_id = ?");
            values.push(warehouse_id.into());
        }
        db.query_one(Statement::from_sql_and_values(DbBackend::Sqlite, sql, values))
            .await?
            .and_then(|row| row.try_get::<f64>("", "incoming_qty").ok())
            .unwrap_or(0.0)
    } else {
        0.0
    };

    let current_on_hand: f64 = balance_row.try_get("", "on_hand_qty")?;

    Ok(StockImpactProjection {
        article_id,
        article_code: balance_row.try_get("", "article_code")?,
        article_name: balance_row.try_get("", "article_name")?,
        warehouse_id,
        warehouse_code,
        current_on_hand,
        reserved_qty: balance_row.try_get("", "reserved_qty")?,
        available_qty: balance_row.try_get("", "available_qty")?,
        incoming_open_po_qty,
        delta_qty,
        projected_on_hand: current_on_hand + delta_qty + incoming_open_po_qty,
    })
}

pub async fn suggest_internal_transfer(
    db: &DatabaseConnection,
    article_id: i64,
    target_warehouse_id: i64,
) -> AppResult<Vec<InventoryStockBalance>> {
    // Find warehouses with surplus for this article (available > max_stock or available > reorder_point)
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT sb.id, sb.article_id, a.article_code, a.article_name,
                    sb.warehouse_id, w.code AS warehouse_code, w.name AS warehouse_name,
                    sb.location_id, sl.code AS location_code, sl.name AS location_name,
                    sb.on_hand_qty, sb.reserved_qty, sb.available_qty, sb.moving_avg_unit_cost, sb.updated_at
             FROM stock_balances sb
             JOIN articles a ON a.id = sb.article_id
             JOIN warehouses w ON w.id = sb.warehouse_id
             JOIN stock_locations sl ON sl.id = sb.location_id
             WHERE sb.article_id = ?
               AND sb.warehouse_id <> ?
               AND sb.available_qty > a.reorder_point
               AND w.is_active = 1
             ORDER BY sb.available_qty DESC",
            [article_id.into(), target_warehouse_id.into()],
        ))
        .await?;
    rows.into_iter()
        .map(|row| {
            Ok(InventoryStockBalance {
                id: row.try_get("", "id")?,
                article_id: row.try_get("", "article_id")?,
                article_code: row.try_get("", "article_code")?,
                article_name: row.try_get("", "article_name")?,
                warehouse_id: row.try_get("", "warehouse_id")?,
                warehouse_code: row.try_get("", "warehouse_code")?,
                warehouse_name: row.try_get("", "warehouse_name")?,
                location_id: row.try_get("", "location_id")?,
                location_code: row.try_get("", "location_code")?,
                location_name: row.try_get("", "location_name")?,
                on_hand_qty: row.try_get("", "on_hand_qty")?,
                reserved_qty: row.try_get("", "reserved_qty")?,
                available_qty: row.try_get("", "available_qty")?,
                moving_avg_unit_cost: row.try_get("", "moving_avg_unit_cost").ok().flatten(),
                updated_at: row.try_get("", "updated_at")?,
            })
        })
        .collect()
}
