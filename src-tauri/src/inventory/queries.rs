use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait};

use crate::errors::{AppError, AppResult};
use crate::inventory::domain::{
    ArticleFamily, CreateArticleFamilyInput, InventoryArticle, InventoryArticleFilter, InventoryArticleInput,
    CreateStockLocationInput, CreateWarehouseInput, InventoryIssueInput, InventoryReleaseReservationInput,
    InventoryReorderRecommendation, InventoryReserveInput, InventoryReturnInput, InventoryStockAdjustInput,
    InventoryStockBalance, InventoryStockFilter, InventoryTaxCategory, InventoryTaxCategoryInput, InventoryTransaction,
    InventoryTransactionFilter, InventoryTransferInput, StockLocation, StockReservation, StockReservationFilter,
    UpdateArticleFamilyInput, UpdateStockLocationInput, UpdateWarehouseInput, Warehouse,
};

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
                   a.preferred_warehouse_id, pw.code AS preferred_warehouse_code,
                   a.preferred_location_id, pl.code AS preferred_location_code,
                   a.min_stock, a.max_stock, a.reorder_point, a.safety_stock, a.is_active, a.row_version, a.created_at, a.updated_at
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
                   a.preferred_warehouse_id, pw.code AS preferred_warehouse_code,
                   a.preferred_location_id, pl.code AS preferred_location_code,
                   a.min_stock, a.max_stock, a.reorder_point, a.safety_stock, a.is_active, a.row_version, a.created_at, a.updated_at
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
                preferred_location_id: row.try_get("", "preferred_location_id")?,
                preferred_location_code: row.try_get("", "preferred_location_code")?,
                min_stock: row.try_get("", "min_stock")?,
                max_stock: row.try_get("", "max_stock")?,
                reorder_point: row.try_get("", "reorder_point")?,
                safety_stock: row.try_get("", "safety_stock")?,
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
            min_stock, max_stock, reorder_point, safety_stock, is_active
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        [
            code.clone().into(),
            name.into(),
            input.family_id.into(),
            input.unit_value_id.into(),
            input.criticality_value_id.into(),
            input.stocking_type_value_id.into(),
            input.tax_category_value_id.into(),
            input.procurement_category_value_id.into(),
            input.preferred_warehouse_id.into(),
            input.preferred_location_id.into(),
            input.min_stock.into(),
            input.max_stock.into(),
            input.reorder_point.into(),
            input.safety_stock.into(),
            is_active.into(),
        ],
    ))
    .await?;
    tx.commit().await?;

    let mut rows = list_articles(
        db,
        InventoryArticleFilter {
            search: Some(code.clone()),
        },
    )
    .await?;

    rows.retain(|r| r.article_code == code);
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
                 is_active = ?,
                 row_version = row_version + 1,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE id = ? AND row_version = ?",
            [
                code.clone().into(),
                name.into(),
                input.family_id.into(),
                input.unit_value_id.into(),
                input.criticality_value_id.into(),
                input.stocking_type_value_id.into(),
                input.tax_category_value_id.into(),
                input.procurement_category_value_id.into(),
                input.preferred_warehouse_id.into(),
                input.preferred_location_id.into(),
                input.min_stock.into(),
                input.max_stock.into(),
                input.reorder_point.into(),
                input.safety_stock.into(),
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

pub async fn list_stock_balances(
    db: &DatabaseConnection,
    filter: InventoryStockFilter,
) -> AppResult<Vec<InventoryStockBalance>> {
    let mut sql = String::from(
        r#"
        SELECT sb.id, sb.article_id, a.article_code, a.article_name,
               sb.warehouse_id, w.code AS warehouse_code,
               sb.location_id, sl.code AS location_code,
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
                location_id: row.try_get("", "location_id")?,
                location_code: row.try_get("", "location_code")?,
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
          sl.id AS location_id,
          sl.code AS location_code,
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
            location_id: row.try_get("", "location_id")?,
            location_code: row.try_get("", "location_code")?,
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
    let source_ref = input.reason.clone();

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
        source_ref.as_deref(),
        input.reason.as_deref(),
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
    reason: Option<&str>,
) -> AppResult<()> {
    db.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO inventory_transactions (
             article_id, warehouse_id, location_id, reservation_id, movement_type, quantity,
             source_type, source_id, source_ref, reason, performed_by_id, performed_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, strftime('%Y-%m-%dT%H:%M:%SZ','now'))",
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
            reason.map(|v| v.to_string()).into(),
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
                    sb.warehouse_id, w.code AS warehouse_code,
                    sb.location_id, sl.code AS location_code,
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
        location_id: row.try_get("", "location_id")?,
        location_code: row.try_get("", "location_code")?,
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
                    r.warehouse_id, w.code AS warehouse_code,
                    r.location_id, sl.code AS location_code,
                    r.source_type, r.source_id, r.source_ref,
                    r.quantity_reserved, r.quantity_issued, r.status, r.notes,
                    r.created_by_id, r.created_at, r.updated_at, r.released_at
             FROM stock_reservations r
             JOIN articles a ON a.id = r.article_id
             JOIN warehouses w ON w.id = r.warehouse_id
             JOIN stock_locations sl ON sl.id = r.location_id
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
        location_id: row.try_get("", "location_id")?,
        location_code: row.try_get("", "location_code")?,
        source_type: row.try_get("", "source_type")?,
        source_id: row.try_get("", "source_id")?,
        source_ref: row.try_get("", "source_ref")?,
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
                r.warehouse_id, w.code AS warehouse_code,
                r.location_id, sl.code AS location_code,
                r.source_type, r.source_id, r.source_ref,
                r.quantity_reserved, r.quantity_issued, r.status, r.notes,
                r.created_by_id, r.created_at, r.updated_at, r.released_at
         FROM stock_reservations r
         JOIN articles a ON a.id = r.article_id
         JOIN warehouses w ON w.id = r.warehouse_id
         JOIN stock_locations sl ON sl.id = r.location_id
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
                location_id: row.try_get("", "location_id")?,
                location_code: row.try_get("", "location_code")?,
                source_type: row.try_get("", "source_type")?,
                source_id: row.try_get("", "source_id")?,
                source_ref: row.try_get("", "source_ref")?,
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
                t.warehouse_id, w.code AS warehouse_code,
                t.location_id, sl.code AS location_code,
                t.reservation_id, t.movement_type, t.quantity,
                t.source_type, t.source_id, t.source_ref,
                t.reason, t.performed_by_id, t.performed_at
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
                location_id: row.try_get("", "location_id")?,
                location_code: row.try_get("", "location_code")?,
                reservation_id: row.try_get("", "reservation_id")?,
                movement_type: row.try_get("", "movement_type")?,
                quantity: row.try_get("", "quantity")?,
                source_type: row.try_get("", "source_type")?,
                source_id: row.try_get("", "source_id")?,
                source_ref: row.try_get("", "source_ref")?,
                reason: row.try_get("", "reason")?,
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
                COALESCE(SUM(sb.on_hand_qty), 0) AS on_hand_qty,
                COALESCE(SUM(sb.reserved_qty), 0) AS reserved_qty,
                COALESCE(SUM(sb.available_qty), 0) AS available_qty
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