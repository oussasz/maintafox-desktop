use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait, Value};

use crate::errors::{AppError, AppResult};
use crate::inventory::domain::{
    ArticleFamily, CreateArticleFamilyInput, InventoryArticle, InventoryArticleFilter, InventoryArticleInput,
    CreateStockLocationInput, CreateWarehouseInput, InventoryIssueInput, InventoryReleaseReservationInput,
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
            "Stock thresholds cannot be negative.".to_string(),
        ]));
    }
    if let Some(max_stock) = input.max_stock {
        if max_stock < 0.0 {
            return Err(AppError::ValidationFailed(vec![
                "max_stock cannot be negative.".to_string(),
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
        return Err(AppError::ValidationFailed(vec![format!("{field_name} does not exist.")]));
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
        return Err(AppError::ValidationFailed(vec!["family_id does not exist.".to_string()]));
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
            "Family code and name are required.".to_string(),
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
            "Family code and name are required.".to_string(),
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

pub async fn deactivate_article_family(
    db: &DatabaseConnection,
    family_id: i64,
) -> AppResult<ArticleFamily> {
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
    let next_sort_order: i64 = row
        .as_ref()
        .and_then(|r| r.try_get("", "next_sort").ok())
        .unwrap_or(1);

    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO lookup_values (
            sync_id, domain_id, code, label, fr_label, en_label, description,
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
            "Tax category code already exists.".to_string(),
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
            [
                tax_category_id.into(),
                domain_id.into(),
                expected_row_version.into(),
            ],
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

pub async fn list_locations(
    db: &DatabaseConnection,
    warehouse_id: Option<i64>,
) -> AppResult<Vec<StockLocation>> {
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

pub async fn create_warehouse(
    db: &DatabaseConnection,
    input: CreateWarehouseInput,
) -> AppResult<Warehouse> {
    let code = input.code.trim().to_string();
    let name = input.name.trim().to_string();
    if code.is_empty() || name.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Warehouse code and name are required.".to_string(),
        ]));
    }
    let dup = db
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
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO warehouses (code, name, is_active) VALUES (?, ?, 1)",
        [code.into(), name.into()],
    ))
    .await?;
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, code, name, is_active, created_at FROM warehouses WHERE id = last_insert_rowid()"
                .to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("warehouse insert")))?;
    Ok(Warehouse {
        id: row.try_get("", "id")?,
        code: row.try_get("", "code")?,
        name: row.try_get("", "name")?,
        is_active: row.try_get("", "is_active")?,
        created_at: row.try_get("", "created_at")?,
    })
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
                "Warehouse name cannot be empty.".to_string(),
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
            "No warehouse fields to update.".to_string(),
        ]));
    }
    vals.push(warehouse_id.into());
    let sql = format!("UPDATE warehouses SET {} WHERE id = ?", sets.join(", "));
    let result = db
        .execute(Statement::from_sql_and_values(DbBackend::Sqlite, sql, vals))
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound {
            entity: "Warehouse".to_string(),
            id: warehouse_id.to_string(),
        });
    }
    let row = db
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
    Ok(Warehouse {
        id: row.try_get("", "id")?,
        code: row.try_get("", "code")?,
        name: row.try_get("", "name")?,
        is_active: row.try_get("", "is_active")?,
        created_at: row.try_get("", "created_at")?,
    })
}

pub async fn create_stock_location(
    db: &DatabaseConnection,
    input: CreateStockLocationInput,
) -> AppResult<StockLocation> {
    let code = input.code.trim().to_string();
    let name = input.name.trim().to_string();
    if code.is_empty() || name.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Location code and name are required.".to_string(),
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
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO stock_locations (warehouse_id, code, name, is_default, is_active, row_version)
         VALUES (?, ?, ?, ?, 1, 1)",
        [
            input.warehouse_id.into(),
            code.into(),
            name.into(),
            i64::from(make_default).into(),
        ],
    ))
    .await?;
    tx.commit().await?;

    let id_row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id FROM stock_locations WHERE rowid = last_insert_rowid()".to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("location insert")))?;
    let new_id: i64 = id_row.try_get("", "id")?;
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
                "Location code cannot be empty.".to_string(),
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
                "Location name cannot be empty.".to_string(),
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
            "No location fields to update.".to_string(),
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

pub async fn list_articles(
    db: &DatabaseConnection,
    filter: InventoryArticleFilter,
) -> AppResult<Vec<InventoryArticle>> {
    let rows = if let Some(search) = filter.search {
        let q = format!("%{}%", search.trim().to_lowercase());
        db.query_all(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            r#"
            SELECT a.id, a.article_code, a.article_name, a.family_id, af.code AS family_code, af.name AS family_name,
                   a.unit_value_id, u.code AS unit_code, u.label AS unit_label,
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
                   a.is_active, a.row_version, a.created_at, a.updated_at
            FROM articles a
            LEFT JOIN article_families af ON af.id = a.family_id
            JOIN lookup_values u ON u.id = a.unit_value_id
            LEFT JOIN lookup_values c ON c.id = a.criticality_value_id
            LEFT JOIN lookup_values st ON st.id = a.stocking_type_value_id
            LEFT JOIN lookup_values tx ON tx.id = a.tax_category_value_id
            LEFT JOIN lookup_values pc ON pc.id = a.procurement_category_value_id
            LEFT JOIN warehouses pw ON pw.id = a.preferred_warehouse_id
            LEFT JOIN stock_locations pl ON pl.id = a.preferred_location_id
            WHERE LOWER(a.article_code) LIKE ? OR LOWER(a.article_name) LIKE ?
            ORDER BY a.article_code ASC
            "#,
            [q.clone().into(), q.into()],
        ))
        .await?
    } else {
        db.query_all(Statement::from_string(
            DbBackend::Sqlite,
            r#"
            SELECT a.id, a.article_code, a.article_name, a.family_id, af.code AS family_code, af.name AS family_name,
                   a.unit_value_id, u.code AS unit_code, u.label AS unit_label,
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
                   a.is_active, a.row_version, a.created_at, a.updated_at
            FROM articles a
            LEFT JOIN article_families af ON af.id = a.family_id
            JOIN lookup_values u ON u.id = a.unit_value_id
            LEFT JOIN lookup_values c ON c.id = a.criticality_value_id
            LEFT JOIN lookup_values st ON st.id = a.stocking_type_value_id
            LEFT JOIN lookup_values tx ON tx.id = a.tax_category_value_id
            LEFT JOIN lookup_values pc ON pc.id = a.procurement_category_value_id
            LEFT JOIN warehouses pw ON pw.id = a.preferred_warehouse_id
            LEFT JOIN stock_locations pl ON pl.id = a.preferred_location_id
            ORDER BY a.article_code ASC
            "#
            .to_string(),
        ))
        .await?
    };

    rows.into_iter()
        .map(|row| {
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
                is_active: row.try_get("", "is_active")?,
                row_version: row.try_get("", "row_version")?,
                created_at: row.try_get("", "created_at")?,
                updated_at: row.try_get("", "updated_at")?,
            })
        })
        .collect()
}

pub async fn create_article(db: &DatabaseConnection, input: InventoryArticleInput) -> AppResult<InventoryArticle> {
    ensure_lookup_value_in_domain(db, input.unit_value_id, "inventory.unit_of_measure", "unit_value_id").await?;
    if let Some(criticality_id) = input.criticality_value_id {
        ensure_lookup_value_in_domain(
            db,
            criticality_id,
            "equipment.criticality",
            "criticality_value_id",
        )
        .await?;
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
    ensure_preferred_location_hint(
        db,
        input.preferred_warehouse_id,
        input.preferred_location_id,
    )
    .await?;

    if input.article_code.trim().is_empty() || input.article_name.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Article code and name are required.".to_string(),
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
            "Article code already exists.".to_string(),
        ]));
    }

    let tx = db.begin().await?;
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO articles (
            article_code, article_name, family_id, unit_value_id, criticality_value_id,
            stocking_type_value_id, tax_category_value_id, procurement_category_value_id,
            preferred_warehouse_id, preferred_location_id,
            min_stock, max_stock, reorder_point, safety_stock,
            manufacturer_name, manufacturer_part_number, oem_part_number,
            replenishment_policy_code, economic_order_qty, minimum_order_qty, maximum_order_qty,
            order_multiple_qty, lead_time_days, review_period_days,
            abc_class_code, xyz_class_code,
            is_critical_spare, requires_expiration, shelf_life_days, requires_batch_tracking,
            is_active
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        [
            code.clone().into(),
            name.into(),
            input.family_id.map_or(Value::BigInt(None), Value::from),
            input.unit_value_id.into(),
            input.criticality_value_id.map_or(Value::BigInt(None), Value::from),
            input.stocking_type_value_id.into(),
            input.tax_category_value_id.into(),
            input.procurement_category_value_id.map_or(Value::BigInt(None), Value::from),
            input.preferred_warehouse_id.map_or(Value::BigInt(None), Value::from),
            input.preferred_location_id.map_or(Value::BigInt(None), Value::from),
            input.min_stock.into(),
            input.max_stock.map_or(Value::Double(None), Value::from),
            input.reorder_point.into(),
            input.safety_stock.into(),
            input.manufacturer_name.clone().map_or(Value::String(None), Value::from),
            input.manufacturer_part_number.clone().map_or(Value::String(None), Value::from),
            input.oem_part_number.clone().map_or(Value::String(None), Value::from),
            input.replenishment_policy_code.clone().map_or(Value::String(None), Value::from),
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
    ensure_lookup_value_in_domain(db, input.unit_value_id, "inventory.unit_of_measure", "unit_value_id").await?;
    if let Some(criticality_id) = input.criticality_value_id {
        ensure_lookup_value_in_domain(
            db,
            criticality_id,
            "equipment.criticality",
            "criticality_value_id",
        )
        .await?;
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
    ensure_preferred_location_hint(
        db,
        input.preferred_warehouse_id,
        input.preferred_location_id,
    )
    .await?;
    validate_article_stock_contract(&input)?;

    let code = input.article_code.trim().to_uppercase();
    let name = input.article_name.trim().to_string();
    if code.is_empty() || name.is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "Article code and name are required.".to_string(),
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
            "Article code already exists.".to_string(),
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
                input.procurement_category_value_id.map_or(Value::BigInt(None), Value::from),
                input.preferred_warehouse_id.map_or(Value::BigInt(None), Value::from),
                input.preferred_location_id.map_or(Value::BigInt(None), Value::from),
                input.min_stock.into(),
                input.max_stock.map_or(Value::Double(None), Value::from),
                input.reorder_point.into(),
                input.safety_stock.into(),
                input.manufacturer_name.clone().map_or(Value::String(None), Value::from),
                input.manufacturer_part_number.clone().map_or(Value::String(None), Value::from),
                input.oem_part_number.clone().map_or(Value::String(None), Value::from),
                input.replenishment_policy_code.clone().map_or(Value::String(None), Value::from),
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
        changes.push((
            "field.article_code",
            format!("{} → {}", before.article_code, new_code),
        ));
    }
    if before.article_name != new_name {
        changes.push((
            "field.article_name",
            format!("{} → {}", before.article_name, new_name),
        ));
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
        changes.push((
            "field.unit",
            format!("{} → updated", before.unit_label),
        ));
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
        changes.push((
            "field.tax_category",
            format!("{} → updated", before.tax_category_label),
        ));
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
        changes.push((
            "field.min_stock",
            format!("{} → {}", before.min_stock, input.min_stock),
        ));
    }
    if before.max_stock != input.max_stock {
        changes.push((
            "field.max_stock",
            format!(
                "{} → {}",
                opt_f64(before.max_stock),
                opt_f64(input.max_stock)
            ),
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
        changes.push((
            "field.is_active",
            format!("{} → {}", before.is_active, new_is_active),
        ));
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
               sb.on_hand_qty, sb.reserved_qty, sb.available_qty, sb.updated_at
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
    if filter.low_stock_only.unwrap_or(false) {
        sql.push_str(" AND a.reorder_point > 0 AND sb.available_qty <= a.reorder_point");
    }
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
    if filter.low_stock_only.unwrap_or(false) {
        syn_sql.push_str(" AND a.reorder_point > 0");
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
            updated_at: row.try_get("", "updated_at")?,
        });
    }

    balances.sort_by(|a, b| {
        a.article_code
            .cmp(&b.article_code)
            .then_with(|| a.warehouse_code.cmp(&b.warehouse_code))
            .then_with(|| a.location_code.cmp(&b.location_code))
    });

    Ok(balances)
}

pub async fn adjust_stock(
    db: &DatabaseConnection,
    input: InventoryStockAdjustInput,
) -> AppResult<InventoryStockBalance> {
    if input.delta_qty == 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "delta_qty must be different from 0.".to_string(),
        ]));
    }
    let source_type = "MANUAL_ADJUSTMENT".to_string();

    let tx = db.begin().await?;
    let warehouse_id = ensure_active_mutation_context(&tx, input.article_id, input.location_id).await?;
    let (on_hand, reserved) = get_balance_snapshot(&tx, input.article_id, input.location_id).await?;
    let next_on_hand = on_hand + input.delta_qty;
    let next_available = next_on_hand - reserved;
    if next_on_hand < 0.0 || next_available < 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "Adjustment would produce negative stock.".to_string(),
        ]));
    }

    // Critical spare guard: block adjustments that would leave on_hand = 0
    if next_on_hand == 0.0 {
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

    insert_inventory_transaction(
        &tx,
        input.article_id,
        warehouse_id,
        input.location_id,
        None,
        if input.delta_qty >= 0.0 {
            "ADJUST_IN"
        } else {
            "ADJUST_OUT"
        },
        input.delta_qty.abs(),
        &source_type,
        None,
        input.source_ref.as_deref(),
        input.reason_code.as_deref(),
        input.notes.as_deref(),
    )
    .await?;

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

async fn get_balance_snapshot<C: ConnectionTrait>(
    db: &C,
    article_id: i64,
    location_id: i64,
) -> AppResult<(f64, f64)> {
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
    if on_hand_qty < 0.0 || reserved_qty < 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "Stock balances cannot be negative.".to_string(),
        ]));
    }
    let available_qty = on_hand_qty - reserved_qty;
    if available_qty < 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "available_qty cannot be negative.".to_string(),
        ]));
    }

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
) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inventory_transactions (
             article_id, warehouse_id, location_id, reservation_id, movement_type, quantity,
             source_type, source_id, source_ref, reason_code, reason, performed_by_id, performed_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
        [
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
    Ok(())
}

async fn load_balance(
    db: &DatabaseConnection,
    article_id: i64,
    location_id: i64,
) -> AppResult<InventoryStockBalance> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT sb.id, sb.article_id, a.article_code, a.article_name,
                    sb.warehouse_id, w.code AS warehouse_code, w.name AS warehouse_name,
                    sb.location_id, sl.code AS location_code, sl.name AS location_name,
                    sb.on_hand_qty, sb.reserved_qty, sb.available_qty, sb.updated_at
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

pub async fn reserve_stock(
    db: &DatabaseConnection,
    input: InventoryReserveInput,
) -> AppResult<StockReservation> {
    if input.quantity <= 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "quantity must be greater than 0.".to_string(),
        ]));
    }
    if input.source_type.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec![
            "source_type is required.".to_string(),
        ]));
    }
    let tx = db.begin().await?;
    let warehouse_id = ensure_active_mutation_context(&tx, input.article_id, input.location_id).await?;
    let (on_hand, reserved) = get_balance_snapshot(&tx, input.article_id, input.location_id).await?;
    let available = on_hand - reserved;
    if available < input.quantity {
        return Err(AppError::ValidationFailed(vec![format!(
            "Insufficient available stock for reservation (available: {available})."
        )]));
    }

    upsert_balance(
        &tx,
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
             article_id, warehouse_id, location_id, source_type, source_id, source_ref,
             quantity_reserved, quantity_issued, status, notes, created_by_id
         ) VALUES (?, ?, ?, ?, ?, ?, ?, 0, 'active', ?, NULL)",
        [
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
        &tx,
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
    )
    .await?;

    tx.commit().await?;
    load_reservation_by_id(db, reservation_id).await
}

pub async fn issue_reserved_stock(
    db: &DatabaseConnection,
    input: InventoryIssueInput,
) -> AppResult<StockReservation> {
    if input.quantity <= 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "quantity must be greater than 0.".to_string(),
        ]));
    }
    let tx = db.begin().await?;
    let reservation = load_reservation_by_id(&tx, input.reservation_id).await?;
    ensure_reservation_invariants(&reservation)?;
    ensure_active_mutation_context(&tx, reservation.article_id, reservation.location_id).await?;
    if reservation.status == "released" {
        return Err(AppError::ValidationFailed(vec![
            "Reservation already released.".to_string(),
        ]));
    }
    let remaining_reserved = reservation.quantity_reserved - reservation.quantity_issued;
    if remaining_reserved < input.quantity {
        return Err(AppError::ValidationFailed(vec![format!(
            "Issue exceeds reserved remainder ({remaining_reserved})."
        )]));
    }

    let (on_hand, reserved) = get_balance_snapshot(&tx, reservation.article_id, reservation.location_id).await?;
    if on_hand < input.quantity || reserved < input.quantity {
        return Err(AppError::ValidationFailed(vec![
            "Insufficient on-hand/reserved quantity for issue.".to_string(),
        ]));
    }

    let next_issued = reservation.quantity_issued + input.quantity;
    let next_status = if next_issued >= reservation.quantity_reserved {
        "consumed"
    } else {
        "partial"
    };

    upsert_balance(
        &tx,
        reservation.article_id,
        reservation.warehouse_id,
        reservation.location_id,
        on_hand - input.quantity,
        reserved - input.quantity,
    )
    .await?;

    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE stock_reservations
         SET quantity_issued = ?, status = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE id = ?",
        [next_issued.into(), next_status.to_string().into(), reservation.id.into()],
    ))
    .await?;

    let tx_source_type = input
        .source_type
        .clone()
        .unwrap_or_else(|| reservation.source_type.clone());
    insert_inventory_transaction(
        &tx,
        reservation.article_id,
        reservation.warehouse_id,
        reservation.location_id,
        Some(reservation.id),
        "ISSUE",
        input.quantity,
        &tx_source_type,
        input.source_id.or(reservation.source_id),
        input.source_ref.as_deref().or(reservation.source_ref.as_deref()),
        None,
        input.notes.as_deref(),
    )
    .await?;

    tx.commit().await?;
    load_reservation_by_id(db, reservation.id).await
}

pub async fn return_reserved_stock(
    db: &DatabaseConnection,
    input: InventoryReturnInput,
) -> AppResult<StockReservation> {
    if input.quantity <= 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "quantity must be greater than 0.".to_string(),
        ]));
    }
    let tx = db.begin().await?;
    let reservation = load_reservation_by_id(&tx, input.reservation_id).await?;
    ensure_reservation_invariants(&reservation)?;
    ensure_active_mutation_context(&tx, reservation.article_id, reservation.location_id).await?;
    if reservation.status == "released" {
        return Err(AppError::ValidationFailed(vec![
            "Cannot return stock for a released reservation.".to_string(),
        ]));
    }
    if reservation.quantity_issued < input.quantity {
        return Err(AppError::ValidationFailed(vec![
            "Return quantity exceeds issued quantity.".to_string(),
        ]));
    }

    let (on_hand, reserved) = get_balance_snapshot(&tx, reservation.article_id, reservation.location_id).await?;
    let next_issued = reservation.quantity_issued - input.quantity;
    let next_status = if reservation.status == "released" {
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
        reserved + input.quantity,
    )
    .await?;
    tx.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "UPDATE stock_reservations
         SET quantity_issued = ?, status = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
         WHERE id = ?",
        [next_issued.into(), next_status.to_string().into(), reservation.id.into()],
    ))
    .await?;
    insert_inventory_transaction(
        &tx,
        reservation.article_id,
        reservation.warehouse_id,
        reservation.location_id,
        Some(reservation.id),
        "RETURN",
        input.quantity,
        &reservation.source_type,
        reservation.source_id,
        reservation.source_ref.as_deref(),
        None,
        input.notes.as_deref(),
    )
    .await?;
    tx.commit().await?;
    load_reservation_by_id(db, reservation.id).await
}

pub(crate) async fn release_stock_reservation_with_connection<C: ConnectionTrait>(
    db: &C,
    reservation_id: i64,
    notes: Option<&str>,
) -> AppResult<StockReservation> {
    let reservation = load_reservation_by_id(db, reservation_id).await?;
    ensure_reservation_invariants(&reservation)?;
    ensure_active_mutation_context(db, reservation.article_id, reservation.location_id).await?;
    if reservation.status == "released" {
        return Ok(reservation);
    }
    let remaining_reserved = (reservation.quantity_reserved - reservation.quantity_issued).max(0.0);

    let (on_hand, reserved) = get_balance_snapshot(db, reservation.article_id, reservation.location_id).await?;
    if reserved < remaining_reserved {
        return Err(AppError::ValidationFailed(vec![
            "Inconsistent reservation state for release.".to_string(),
        ]));
    }

    upsert_balance(
        db,
        reservation.article_id,
        reservation.warehouse_id,
        reservation.location_id,
        on_hand,
        reserved - remaining_reserved,
    )
    .await?;
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
    if remaining_reserved > 0.0 {
        insert_inventory_transaction(
            db,
            reservation.article_id,
            reservation.warehouse_id,
            reservation.location_id,
            Some(reservation.id),
            "RELEASE",
            remaining_reserved,
            &reservation.source_type,
            reservation.source_id,
            reservation.source_ref.as_deref(),
            None,
            notes,
        )
        .await?;
    }
    load_reservation_by_id(db, reservation.id).await
}

pub async fn release_stock_reservation(
    db: &DatabaseConnection,
    input: InventoryReleaseReservationInput,
) -> AppResult<StockReservation> {
    let tx = db.begin().await?;
    let released =
        release_stock_reservation_with_connection(&tx, input.reservation_id, input.notes.as_deref()).await?;
    tx.commit().await?;
    Ok(released)
}

pub async fn transfer_stock(
    db: &DatabaseConnection,
    input: InventoryTransferInput,
) -> AppResult<Vec<InventoryStockBalance>> {
    if input.quantity <= 0.0 {
        return Err(AppError::ValidationFailed(vec![
            "quantity must be greater than 0.".to_string(),
        ]));
    }
    if input.from_location_id == input.to_location_id {
        return Err(AppError::ValidationFailed(vec![
            "from_location_id and to_location_id must be different.".to_string(),
        ]));
    }

    let tx = db.begin().await?;
    ensure_article_active(&tx, input.article_id).await?;
    let from_warehouse = ensure_location_exists(&tx, input.from_location_id).await?;
    let to_warehouse = ensure_location_exists(&tx, input.to_location_id).await?;

    let (from_on_hand, from_reserved) =
        get_balance_snapshot(&tx, input.article_id, input.from_location_id).await?;
    let from_available = from_on_hand - from_reserved;
    if from_available < input.quantity {
        return Err(AppError::ValidationFailed(vec![format!(
            "Insufficient available stock at source location ({from_available})."
        )]));
    }

    let (to_on_hand, to_reserved) = get_balance_snapshot(&tx, input.article_id, input.to_location_id).await?;

    upsert_balance(
        &tx,
        input.article_id,
        from_warehouse,
        input.from_location_id,
        from_on_hand - input.quantity,
        from_reserved,
    )
    .await?;
    upsert_balance(
        &tx,
        input.article_id,
        to_warehouse,
        input.to_location_id,
        to_on_hand + input.quantity,
        to_reserved,
    )
    .await?;

    let source_type = input
        .source_type
        .clone()
        .unwrap_or_else(|| "INTERNAL_TRANSFER".to_string());
    insert_inventory_transaction(
        &tx,
        input.article_id,
        from_warehouse,
        input.from_location_id,
        None,
        "TRANSFER_OUT",
        input.quantity,
        &source_type,
        input.source_id,
        input.source_ref.as_deref(),
        None,
        input.notes.as_deref(),
    )
    .await?;
    insert_inventory_transaction(
        &tx,
        input.article_id,
        to_warehouse,
        input.to_location_id,
        None,
        "TRANSFER_IN",
        input.quantity,
        &source_type,
        input.source_id,
        input.source_ref.as_deref(),
        None,
        input.notes.as_deref(),
    )
    .await?;

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
                t.reason_code, t.reason, t.performed_by_id, t.performed_at
         FROM inventory_transactions t
         JOIN articles a ON a.id = t.article_id
         JOIN warehouses w ON w.id = t.warehouse_id
         JOIN stock_locations sl ON sl.id = t.location_id
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
    for r in base {
        let reason = match r.trigger_type.as_str() {
            "reorder_point" => Some("Below reorder point".to_string()),
            "min_stock" => Some("Below minimum stock".to_string()),
            other => Some(format!("Replenishment trigger: {other}")),
        };

        let transfer_balances = suggest_internal_transfer(db, r.article_id, r.warehouse_id).await?;
        let transfer_options: Vec<crate::inventory::domain::ReplenishmentTransferOption> =
            transfer_balances
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
            reason,
            transfer_options,
        });
    }
    Ok(result)
}

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

    // Compute consumption value per article from inventory_transactions over last 12 months
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT t.article_id, COALESCE(SUM(t.quantity), 0.0) AS total_issued
             FROM inventory_transactions t
             WHERE t.movement_type IN ('ISSUE','ADJUST_OUT','GR_ACCEPT')
               AND t.performed_at >= strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-12 months')
             GROUP BY t.article_id
             ORDER BY total_issued DESC"
                .to_string(),
        ))
        .await
        .unwrap_or_default();

    let total: f64 = rows.iter().map(|r| r.try_get::<f64>("", "total_issued").unwrap_or(0.0)).sum();
    if total == 0.0 {
        return Ok(0);
    }

    let mut cumulative = 0.0;
    let mut updated_count = 0i64;
    for row in &rows {
        let article_id: i64 = row.try_get("", "article_id").unwrap_or(0);
        let qty: f64 = row.try_get("", "total_issued").unwrap_or(0.0);
        cumulative += qty;
        let pct = cumulative / total * 100.0;
        let class = if pct <= a_pct { "A" } else if pct <= b_pct { "B" } else { "C" };
        db.execute(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "UPDATE articles SET abc_class_code = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?",
            [class.into(), article_id.into()],
        ))
        .await
        .ok();
        updated_count += 1;
    }
    Ok(updated_count)
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
        let class = if cv <= x_cv_max { "X" } else if cv <= y_cv_max { "Y" } else { "Z" };
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
        return Err(AppError::ValidationFailed(vec![format!(
            "equivalence_type must be one of: DIRECT, FUNCTIONAL, UPGRADE."
        )]));
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

pub async fn delete_article_equivalent(
    db: &DatabaseConnection,
    equivalent_id: i64,
) -> AppResult<()> {
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

pub async fn get_procurement_dashboard_summary(
    db: &DatabaseConnection,
) -> AppResult<crate::inventory::domain::ProcurementDashboardSummary> {
    let row = db
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT
                (SELECT COUNT(*) FROM procurement_requisitions WHERE status NOT IN ('APPROVED','CLOSED','CANCELLED','PARTIALLY_RECEIVED')) AS open_requisitions,
                (SELECT COUNT(*) FROM purchase_orders WHERE status = 'SUBMITTED') AS pending_approval_pos,
                (SELECT COUNT(*) FROM purchase_orders WHERE status IN ('DRAFT','APPROVED','PARTIALLY_RECEIVED')) AS open_pos,
                (SELECT COUNT(*) FROM purchase_orders
                 WHERE status IN ('APPROVED','PARTIALLY_RECEIVED')
                   AND expected_delivery_date IS NOT NULL
                   AND expected_delivery_date < strftime('%Y-%m-%dT%H:%M:%SZ','now')) AS overdue_pos,
                (SELECT COUNT(*) FROM purchase_orders WHERE status IN ('APPROVED','PARTIALLY_RECEIVED')) AS pending_receipts,
                (SELECT COUNT(DISTINCT a.id) FROM articles a
                 JOIN stock_balances sb ON sb.article_id = a.id
                 WHERE a.is_active = 1 AND sb.available_qty <= a.reorder_point AND a.reorder_point > 0) AS low_stock_articles,
                (SELECT COUNT(DISTINCT a.id) FROM articles a
                 JOIN stock_balances sb ON sb.article_id = a.id
                 WHERE a.is_active = 1 AND COALESCE(a.is_critical_spare, 0) = 1
                   AND sb.available_qty <= a.min_stock) AS critical_low_stock_articles,
                (SELECT COUNT(*) FROM repairable_orders
                 WHERE status IN ('REQUESTED','RELEASED','SENT_FOR_REPAIR','RETURNED_FROM_REPAIR')) AS repairables_in_repair,
                (SELECT COUNT(*) FROM inventory_suppliers
                 WHERE is_active = 1 AND status_code <> 'BLOCKED') AS active_suppliers_count,
                (SELECT COUNT(*) FROM purchase_orders
                 WHERE status IN ('APPROVED','PARTIALLY_RECEIVED')
                   AND expected_delivery_date IS NOT NULL
                   AND date(expected_delivery_date) = date('now')) AS receiving_today_count"
                .to_string(),
        ))
        .await?
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("dashboard summary query failed")))?;
    Ok(crate::inventory::domain::ProcurementDashboardSummary {
        open_requisitions: row.try_get("", "open_requisitions").unwrap_or(0),
        pending_approval_pos: row.try_get("", "pending_approval_pos").unwrap_or(0),
        open_pos: row.try_get("", "open_pos").unwrap_or(0),
        overdue_pos: row.try_get("", "overdue_pos").unwrap_or(0),
        pending_receipts: row.try_get("", "pending_receipts").unwrap_or(0),
        low_stock_articles: row.try_get("", "low_stock_articles").unwrap_or(0),
        critical_low_stock_articles: row.try_get("", "critical_low_stock_articles").unwrap_or(0),
        repairables_in_repair: row.try_get("", "repairables_in_repair").unwrap_or(0),
        active_suppliers_count: row.try_get("", "active_suppliers_count").unwrap_or(0),
        receiving_today_count: row.try_get("", "receiving_today_count").unwrap_or(0),
    })
}

/// Operational alerts for the procurement dashboard. Top 3 per kind.
pub async fn get_procurement_alerts(
    db: &DatabaseConnection,
) -> AppResult<Vec<crate::inventory::domain::ProcurementAlert>> {
    let mut alerts = Vec::new();

    let critical_rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT a.id, a.article_code, a.article_name,
                    COALESCE(SUM(sb.available_qty), 0.0) AS available_qty, a.min_stock
             FROM articles a
             JOIN stock_balances sb ON sb.article_id = a.id
             WHERE a.is_active = 1 AND COALESCE(a.is_critical_spare, 0) = 1
               AND a.min_stock > 0
             GROUP BY a.id
             HAVING available_qty <= a.min_stock
             ORDER BY available_qty ASC
             LIMIT 3"
                .to_string(),
        ))
        .await
        .unwrap_or_default();
    for row in critical_rows {
        let code: String = row.try_get("", "article_code").unwrap_or_default();
        let name: String = row.try_get("", "article_name").unwrap_or_default();
        let avail: f64 = row.try_get("", "available_qty").unwrap_or(0.0);
        let min_stock: f64 = row.try_get("", "min_stock").unwrap_or(0.0);
        alerts.push(crate::inventory::domain::ProcurementAlert {
            kind: "CRITICAL_STOCK".to_string(),
            severity: "high".to_string(),
            title: format!("Critical stock: {code}"),
            detail: Some(format!("{name} available {avail} ≤ min {min_stock}")),
            entity_type: Some("ARTICLE".to_string()),
            entity_id: row.try_get("", "id").ok(),
            entity_code: Some(code),
        });
    }

    let late_rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, po_number, expected_delivery_date
             FROM purchase_orders
             WHERE status IN ('APPROVED','PARTIALLY_RECEIVED')
               AND expected_delivery_date IS NOT NULL
               AND expected_delivery_date < strftime('%Y-%m-%dT%H:%M:%SZ','now')
             ORDER BY expected_delivery_date ASC
             LIMIT 3"
                .to_string(),
        ))
        .await
        .unwrap_or_default();
    for row in late_rows {
        let po_number: String = row.try_get("", "po_number").unwrap_or_default();
        let eta: Option<String> = row.try_get("", "expected_delivery_date").ok().flatten();
        alerts.push(crate::inventory::domain::ProcurementAlert {
            kind: "LATE_PO".to_string(),
            severity: "high".to_string(),
            title: format!("Late PO: {po_number}"),
            detail: eta.map(|d| format!("Expected {d}")),
            entity_type: Some("PURCHASE_ORDER".to_string()),
            entity_id: row.try_get("", "id").ok(),
            entity_code: Some(po_number),
        });
    }

    let repair_rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT ro.id, ro.order_code, ro.sent_at, a.article_code,
                    COALESCE(s.default_lead_time_days, a.lead_time_days, 14) AS lead_sla_days
             FROM repairable_orders ro
             JOIN articles a ON a.id = ro.article_id
             LEFT JOIN inventory_suppliers s ON s.id = ro.vendor_supplier_id
             WHERE ro.status = 'SENT_FOR_REPAIR'
               AND ro.sent_at IS NOT NULL
               AND datetime(ro.sent_at, '+' || COALESCE(s.default_lead_time_days, a.lead_time_days, 14) || ' days')
                   < strftime('%Y-%m-%dT%H:%M:%SZ','now')
             ORDER BY ro.sent_at ASC
             LIMIT 3"
                .to_string(),
        ))
        .await
        .unwrap_or_default();
    for row in repair_rows {
        let code: String = row.try_get("", "order_code").unwrap_or_default();
        let article: String = row.try_get("", "article_code").unwrap_or_default();
        let sla: i64 = row.try_get("", "lead_sla_days").unwrap_or(14);
        alerts.push(crate::inventory::domain::ProcurementAlert {
            kind: "REPAIRABLE_OVERDUE".to_string(),
            severity: "medium".to_string(),
            title: format!("Repair overdue: {code}"),
            detail: Some(format!("{article} past {sla}-day lead SLA")),
            entity_type: Some("REPAIRABLE_ORDER".to_string()),
            entity_id: row.try_get("", "id").ok(),
            entity_code: Some(code),
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
        .await
        .unwrap_or_default();

    let mut high_risk = Vec::new();
    for row in supplier_rows {
        let total: i64 = row.try_get("", "total_lines").unwrap_or(0);
        if total <= 0 {
            continue;
        }
        let on_time: i64 = row.try_get("", "on_time_lines").unwrap_or(0);
        let avg_lt: Option<f64> = row.try_get("", "avg_lead_time").ok().flatten();
        let total_recv: f64 = row.try_get("", "total_received").unwrap_or(0.0);
        let total_rej: f64 = row.try_get("", "total_rejected").unwrap_or(0.0);
        let otif = Some(on_time as f64 / total as f64 * 100.0);
        let rejected = if total_recv > 0.0 {
            Some(total_rej / total_recv * 100.0)
        } else {
            None
        };
        let promised: Option<i64> = row.try_get("", "default_lead_time_days").ok().flatten();
        let risk = crate::inventory::suppliers::compute_supplier_risk_level(
            otif, rejected, avg_lt, promised,
        );
        if risk.as_deref() == Some("HIGH") {
            let code: String = row.try_get("", "code").unwrap_or_default();
            let name: String = row.try_get("", "name").unwrap_or_default();
            high_risk.push(crate::inventory::domain::ProcurementAlert {
                kind: "SUPPLIER_DELAY".to_string(),
                severity: "medium".to_string(),
                title: format!("High-risk supplier: {code}"),
                detail: Some(name),
                entity_type: Some("SUPPLIER".to_string()),
                entity_id: row.try_get("", "id").ok(),
                entity_code: Some(code),
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
            [
                article_id.into(),
                format!("-{} months", months).into(),
            ],
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
        return Err(AppError::ValidationFailed(vec!["document_ref is required.".to_string()]));
    }
    if input.link_purpose.trim().is_empty() {
        return Err(AppError::ValidationFailed(vec!["link_purpose is required.".to_string()]));
    }
    let is_primary = input.is_primary.unwrap_or(false) as i64;
    let valid_from = input.valid_from.clone().unwrap_or_else(|| {
        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
    });
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
        return Err(AppError::ValidationFailed(vec!["delta_qty must be a finite number.".to_string()]));
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
        .query_one(Statement::from_sql_and_values(DbBackend::Sqlite, balance_sql, balance_values))
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
                    sb.on_hand_qty, sb.reserved_qty, sb.available_qty, sb.updated_at
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
                updated_at: row.try_get("", "updated_at")?,
            })
        })
        .collect()
}