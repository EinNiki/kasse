//! Tests required by the stock-rules skill: checkout, stock limits, concurrency, change.
mod common;

use common::{count, fixture, stock_of};
use kasse::cart::CartLine;
use kasse::db::sales::{checkout, CheckoutError};
use kasse::db::stock::{adjust, StockError, REASON_CORRECTION, REASON_RECEIPT};
use kasse::db::{self};
use sqlx::PgPool;

fn line(product_id: i64, quantity: i32) -> CartLine {
    CartLine {
        product_id,
        quantity,
    }
}

#[sqlx::test]
async fn checkout_with_enough_stock_reduces_stock(pool: PgPool) {
    let f = fixture(&pool, 5).await;
    let receipt = checkout(&pool, f.user_id, &[line(f.beer, 2), line(f.cola, 1)], 1000)
        .await
        .unwrap();

    assert_eq!(receipt.total_cents, 800);
    assert_eq!(receipt.change_cents, 200);
    assert_eq!(stock_of(&pool, f.beer).await, 3);
    assert_eq!(stock_of(&pool, f.cola).await, 9);
    assert_eq!(count(&pool, "sales").await, 1);
    assert_eq!(count(&pool, "sale_items").await, 2);

    // negative movements are written for every position
    let deltas: Vec<i32> = sqlx::query_scalar(
        "SELECT delta FROM stock_movements WHERE reason = 'Verkauf' ORDER BY product_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(deltas, vec![-2, -1]);
}

#[sqlx::test]
async fn checkout_with_too_little_stock_fails_and_changes_nothing(pool: PgPool) {
    let f = fixture(&pool, 1).await;
    let movements_before = count(&pool, "stock_movements").await;

    // cola would be fine, beer is not: nothing may be booked at all
    let err = checkout(&pool, f.user_id, &[line(f.cola, 1), line(f.beer, 2)], 5000)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            CheckoutError::Insufficient {
                available: 1,
                requested: 2,
                ..
            }
        ),
        "{err:?}"
    );

    assert_eq!(stock_of(&pool, f.beer).await, 1);
    assert_eq!(stock_of(&pool, f.cola).await, 10);
    assert_eq!(count(&pool, "sales").await, 0);
    assert_eq!(count(&pool, "sale_items").await, 0);
    assert_eq!(count(&pool, "stock_movements").await, movements_before);
}

#[sqlx::test]
async fn two_parallel_checkouts_for_the_last_item_only_one_succeeds(pool: PgPool) {
    let f = fixture(&pool, 1).await;
    let spawn = |pool: PgPool| {
        tokio::spawn(async move { checkout(&pool, f.user_id, &[line(f.beer, 1)], 300).await })
    };
    let (a, b) = (spawn(pool.clone()), spawn(pool.clone()));
    let (a, b) = (a.await.unwrap(), b.await.unwrap());
    let successes = [a.is_ok(), b.is_ok()].into_iter().filter(|ok| *ok).count();
    assert_eq!(successes, 1, "exactly one checkout may win");
    let loser = if a.is_err() { a } else { b };
    assert!(matches!(loser, Err(CheckoutError::Insufficient { .. })));
    assert_eq!(stock_of(&pool, f.beer).await, 0);
    assert_eq!(count(&pool, "sales").await, 1);
}

#[sqlx::test]
async fn many_parallel_checkouts_never_oversell(pool: PgPool) {
    let f = fixture(&pool, 5).await;
    let mut tasks = Vec::new();
    for _ in 0..12 {
        let pool = pool.clone();
        tasks.push(tokio::spawn(async move {
            checkout(&pool, f.user_id, &[line(f.beer, 1)], 300)
                .await
                .is_ok()
        }));
    }
    let mut ok = 0;
    for t in tasks {
        ok += usize::from(t.await.unwrap());
    }
    assert_eq!(ok, 5);
    assert_eq!(stock_of(&pool, f.beer).await, 0);
}

#[sqlx::test]
async fn change_is_paid_minus_total_including_exact_amount(pool: PgPool) {
    let f = fixture(&pool, 5).await;
    let exact = checkout(&pool, f.user_id, &[line(f.beer, 1)], 300)
        .await
        .unwrap();
    assert_eq!(exact.change_cents, 0, "exactly the same amount");
    let more = checkout(&pool, f.user_id, &[line(f.beer, 1)], 1000)
        .await
        .unwrap();
    assert_eq!(more.change_cents, 700);
    let stored: i64 = sqlx::query_scalar("SELECT change_cents FROM sales WHERE id = $1")
        .bind(more.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stored, 700);
}

#[sqlx::test]
async fn underpaid_checkout_books_nothing(pool: PgPool) {
    let f = fixture(&pool, 5).await;
    let err = checkout(&pool, f.user_id, &[line(f.beer, 1)], 299)
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        CheckoutError::Underpaid {
            total_cents: 300,
            paid_cents: 299
        }
    ));
    assert_eq!(stock_of(&pool, f.beer).await, 5);
    assert_eq!(count(&pool, "sales").await, 0);
}

#[sqlx::test]
async fn empty_cart_and_deleted_product_are_rejected(pool: PgPool) {
    let f = fixture(&pool, 5).await;
    assert!(matches!(
        checkout(&pool, f.user_id, &[], 100).await,
        Err(CheckoutError::EmptyCart)
    ));
    assert!(matches!(
        checkout(&pool, f.user_id, &[line(f.beer, 0)], 100).await,
        Err(CheckoutError::EmptyCart)
    ));

    db::products::soft_delete(&pool, f.beer).await.unwrap();
    let err = checkout(&pool, f.user_id, &[line(f.beer, 1)], 300)
        .await
        .unwrap_err();
    assert!(matches!(err, CheckoutError::UnknownProduct(_)));
    assert_eq!(stock_of(&pool, f.beer).await, 5);
}

#[sqlx::test]
async fn duplicate_cart_lines_are_merged_for_the_stock_check(pool: PgPool) {
    let f = fixture(&pool, 3).await;
    let err = checkout(&pool, f.user_id, &[line(f.beer, 2), line(f.beer, 2)], 5000)
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        CheckoutError::Insufficient {
            available: 3,
            requested: 4,
            ..
        }
    ));
    assert_eq!(stock_of(&pool, f.beer).await, 3);
}

#[sqlx::test]
async fn price_change_only_affects_new_sales(pool: PgPool) {
    let f = fixture(&pool, 5).await;
    let old = checkout(&pool, f.user_id, &[line(f.beer, 1)], 300)
        .await
        .unwrap();
    db::products::update(&pool, f.beer, f.category_id, "Bier neu", 450, false)
        .await
        .unwrap();
    let new = checkout(&pool, f.user_id, &[line(f.beer, 1)], 500)
        .await
        .unwrap();

    assert_eq!(new.total_cents, 450);
    let stored = db::sales::get_receipt(&pool, old.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.total_cents, 300);
    assert_eq!(stored.items[0].unit_price_cents, 300);
    assert_eq!(stored.items[0].name, "Bier", "name snapshot stays as sold");
}

#[sqlx::test]
async fn receipt_is_stored_with_snapshot(pool: PgPool) {
    let f = fixture(&pool, 5).await;
    let r = checkout(&pool, f.user_id, &[line(f.beer, 2)], 1000)
        .await
        .unwrap();
    let again = db::sales::get_receipt(&pool, r.id).await.unwrap().unwrap();
    assert_eq!(
        (again.total_cents, again.paid_cents, again.change_cents),
        (600, 1000, 400)
    );
    assert_eq!(again.items.len(), 1);
    assert!(db::sales::get_receipt(&pool, r.id + 1000)
        .await
        .unwrap()
        .is_none());
}

// ---------- goods receipt and correction ----------

#[sqlx::test]
async fn receipt_and_correction_are_logged_with_reason_and_comment(pool: PgPool) {
    let f = fixture(&pool, 0).await;
    assert_eq!(
        adjust(
            &pool,
            f.beer,
            24,
            REASON_RECEIPT,
            Some("Lieferung"),
            f.user_id
        )
        .await
        .unwrap(),
        24
    );
    assert_eq!(
        adjust(
            &pool,
            f.beer,
            -4,
            REASON_CORRECTION,
            Some("Bruch"),
            f.user_id
        )
        .await
        .unwrap(),
        20
    );

    let rows = db::stock::movements(&pool, Some(f.beer), 10).await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        (
            rows[0].delta,
            rows[0].reason.as_str(),
            rows[0].comment.as_deref()
        ),
        (-4, "Korrektur", Some("Bruch"))
    );
    assert_eq!(
        (
            rows[1].delta,
            rows[1].reason.as_str(),
            rows[1].comment.as_deref()
        ),
        (24, "Wareneingang", Some("Lieferung"))
    );
    assert_eq!(rows[0].username, "kasse");
}

#[sqlx::test]
async fn correction_can_never_make_stock_negative(pool: PgPool) {
    let f = fixture(&pool, 3).await;
    let before = count(&pool, "stock_movements").await;
    let err = adjust(&pool, f.beer, -4, REASON_CORRECTION, None, f.user_id)
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        StockError::Insufficient {
            available: 3,
            requested: 4
        }
    ));
    assert_eq!(stock_of(&pool, f.beer).await, 3);
    assert_eq!(
        count(&pool, "stock_movements").await,
        before,
        "failed change must not be logged"
    );

    // exactly to zero is allowed
    assert_eq!(
        adjust(&pool, f.beer, -3, REASON_CORRECTION, None, f.user_id)
            .await
            .unwrap(),
        0
    );
}

#[sqlx::test]
async fn zero_and_overflowing_quantities_are_rejected(pool: PgPool) {
    let f = fixture(&pool, 3).await;
    assert!(matches!(
        adjust(&pool, f.beer, 0, REASON_RECEIPT, None, f.user_id).await,
        Err(StockError::InvalidQuantity)
    ));
    assert!(matches!(
        adjust(&pool, f.beer, i32::MAX, REASON_RECEIPT, None, f.user_id).await,
        Err(StockError::InvalidQuantity)
    ));
    assert!(matches!(
        adjust(&pool, 99_999, 1, REASON_RECEIPT, None, f.user_id).await,
        Err(StockError::NotFound)
    ));
}

#[sqlx::test]
async fn database_constraint_blocks_negative_stock_even_with_raw_sql(pool: PgPool) {
    let f = fixture(&pool, 1).await;
    let res = sqlx::query("UPDATE products SET stock = -1 WHERE id = $1")
        .bind(f.beer)
        .execute(&pool)
        .await;
    assert!(res.is_err());
}

// ---------- products and categories ----------

#[sqlx::test]
async fn soft_deleted_products_vanish_from_lists_but_keep_history(pool: PgPool) {
    let f = fixture(&pool, 5).await;
    checkout(&pool, f.user_id, &[line(f.beer, 1)], 300)
        .await
        .unwrap();
    db::products::soft_delete(&pool, f.beer).await.unwrap();

    let names: Vec<String> = db::products::list(&pool, None, None)
        .await
        .unwrap()
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert_eq!(names, vec!["Cola"]);
    assert!(db::products::get(&pool, f.beer).await.unwrap().is_none());
    assert_eq!(count(&pool, "sale_items").await, 1);
    assert!(matches!(
        db::products::soft_delete(&pool, f.beer).await,
        Err(db::products::ProductError::NotFound)
    ));
}

#[sqlx::test]
async fn product_search_and_category_filter(pool: PgPool) {
    let f = fixture(&pool, 0).await;
    let other = db::categories::create(&pool, "Kaffee").await.unwrap();
    db::products::create(&pool, other, "Kaffee Pott 100%", 150, false)
        .await
        .unwrap();

    let by_name = db::products::list(&pool, Some("bIe"), None).await.unwrap();
    assert_eq!(
        by_name.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        vec!["Bier"]
    );
    let by_cat = db::products::list(&pool, None, Some(other)).await.unwrap();
    assert_eq!(by_cat.len(), 1);
    let combined = db::products::list(&pool, Some("cola"), Some(other))
        .await
        .unwrap();
    assert!(combined.is_empty());
    // LIKE wildcards are matched literally
    assert_eq!(
        db::products::list(&pool, Some("100%"), None)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        db::products::list(&pool, Some("%"), None)
            .await
            .unwrap()
            .len()
            == 1
    );
    assert_eq!(
        db::products::list(&pool, None, Some(f.category_id))
            .await
            .unwrap()
            .len(),
        2
    );
}

#[sqlx::test]
async fn product_validation(pool: PgPool) {
    let f = fixture(&pool, 0).await;
    use db::products::{create, ProductError};
    assert!(matches!(
        create(&pool, f.category_id, "  ", 100, false).await,
        Err(ProductError::InvalidName)
    ));
    assert!(matches!(
        create(&pool, f.category_id, "X", -1, false).await,
        Err(ProductError::InvalidPrice)
    ));
    assert!(matches!(
        create(&pool, 9999, "X", 100, false).await,
        Err(ProductError::InvalidCategory)
    ));
}

#[sqlx::test]
async fn category_rules(pool: PgPool) {
    use db::categories::{create, delete, rename, CategoryError};
    let f = fixture(&pool, 0).await;

    assert!(matches!(
        create(&pool, "Getränke").await,
        Err(CategoryError::Duplicate)
    ));
    assert!(matches!(
        create(&pool, "   ").await,
        Err(CategoryError::InvalidName)
    ));

    let snacks = create(&pool, " Snacks ").await.unwrap();
    rename(&pool, snacks, "Speisen").await.unwrap();
    assert!(matches!(
        rename(&pool, snacks, "Getränke").await,
        Err(CategoryError::Duplicate)
    ));
    assert!(matches!(
        rename(&pool, 9999, "X").await,
        Err(CategoryError::NotFound)
    ));
    let names: Vec<String> = db::categories::list(&pool)
        .await
        .unwrap()
        .into_iter()
        .map(|c| c.name)
        .collect();
    assert_eq!(names, vec!["Getränke", "Speisen"]);

    // a category with active products cannot be deleted
    assert!(matches!(
        delete(&pool, f.category_id).await,
        Err(CategoryError::HasProducts(2))
    ));
    // after soft-deleting its products it can; they keep existing without category
    db::products::soft_delete(&pool, f.beer).await.unwrap();
    db::products::soft_delete(&pool, f.cola).await.unwrap();
    delete(&pool, f.category_id).await.unwrap();
    assert!(matches!(
        delete(&pool, f.category_id).await,
        Err(CategoryError::NotFound)
    ));
    delete(&pool, snacks).await.unwrap();
}

// ---------- seed ----------

#[sqlx::test]
async fn seed_inserts_the_full_price_list_once(pool: PgPool) {
    assert_eq!(kasse::seed::run(&pool).await.unwrap(), 28);
    assert_eq!(
        kasse::seed::run(&pool).await.unwrap(),
        0,
        "second run does nothing"
    );

    assert_eq!(count(&pool, "categories").await, 2);
    assert_eq!(count(&pool, "products").await, 28);
    let stock_sum: i64 = sqlx::query_scalar("SELECT COALESCE(SUM(stock), 0)::bigint FROM products")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stock_sum, 0, "stock starts at 0");

    let price: i64 = sqlx::query_scalar(
        "SELECT price_cents FROM products WHERE name = 'Flasche Mineralwasser 0,50 l'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(price, 200);
    let cat: String = sqlx::query_scalar(
        "SELECT c.name FROM products p JOIN categories c ON c.id = p.category_id WHERE p.name LIKE 'Apfelmost pur, groß%'",
    )
    .fetch_one(&pool).await.unwrap();
    assert_eq!(cat, "Alkohol");
    for needle in [
        "Schnaps - Likör 2 cl",
        "Kaffee Pott",
        "Tee versch. Sorten, Tasse",
    ] {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM products WHERE name = $1")
            .bind(needle)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 1, "{needle}");
    }
}

// ---------- statistics ----------

#[sqlx::test]
async fn statistics_aggregate_sales(pool: PgPool) {
    let f = fixture(&pool, 20).await;
    checkout(&pool, f.user_id, &[line(f.beer, 3), line(f.cola, 1)], 2000)
        .await
        .unwrap(); // 11,00 €
    checkout(&pool, f.user_id, &[line(f.cola, 4)], 1000)
        .await
        .unwrap(); // 8,00 €

    let today = db::stats::today(&pool).await.unwrap();
    let totals = db::stats::totals(&pool, today, today).await.unwrap();
    assert_eq!((totals.revenue_cents, totals.sales_count), (1900, 2));

    let products = db::stats::product_stats(&pool, today, today).await.unwrap();
    let cola = products.iter().find(|p| p.name == "Cola").unwrap();
    assert_eq!((cola.quantity, cola.revenue_cents), (5, 1000));

    let cats = db::stats::category_stats(&pool, today, today)
        .await
        .unwrap();
    assert_eq!(cats.len(), 1);
    assert_eq!(
        (cats[0].name.as_str(), cats[0].revenue_cents),
        ("Getränke", 1900)
    );

    let recent = db::stats::recent_sales(&pool, today, today, 10)
        .await
        .unwrap();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].total_cents, 800, "newest first");
    assert!(
        recent[1].items.contains("3× Bier") && recent[1].items.contains("1× Cola"),
        "{}",
        recent[1].items
    );

    // a range in the past is empty
    let past = today - chrono::Duration::days(10);
    assert_eq!(
        db::stats::totals(&pool, past, past)
            .await
            .unwrap()
            .sales_count,
        0
    );
}

#[sqlx::test]
async fn search_puts_the_best_match_first(pool: PgPool) {
    let f = fixture(&pool, 0).await;
    for name in ["Weißbier 0,5 l", "Flasche Bier", "Bier hell"] {
        db::products::create(&pool, f.category_id, name, 300, false)
            .await
            .unwrap();
    }
    let names = |q: &'static str| {
        let pool = pool.clone();
        async move {
            db::products::list(&pool, Some(q), None)
                .await
                .unwrap()
                .into_iter()
                .map(|p| p.name)
                .collect::<Vec<_>>()
        }
    };
    // starts with > a word starts with > contained anywhere (the fixture's plain "Bier" ranks first)
    assert_eq!(
        names("bier").await,
        vec!["Bier", "Bier hell", "Flasche Bier", "Weißbier 0,5 l"]
    );
    assert_eq!(names("fla").await, vec!["Flasche Bier"]);
    assert!(names("zzz").await.is_empty());
    // no term: all products, in category and name order
    assert_eq!(
        db::products::list(&pool, None, None).await.unwrap().len(),
        5
    );
}

// ---------- Unlimited products ----------

#[sqlx::test]
async fn unlimited_product_sells_without_stock_and_is_not_counted(pool: PgPool) {
    let f = fixture(&pool, 0).await;
    let water = db::products::create(&pool, f.category_id, "Leitungswasser", 100, true)
        .await
        .unwrap();
    let movements_before = count(&pool, "stock_movements").await;

    // stock is 0, selling still works, even many units at once
    let receipt = checkout(&pool, f.user_id, &[line(water, 50)], 5000)
        .await
        .unwrap();
    assert_eq!(receipt.total_cents, 5000);
    assert_eq!(
        stock_of(&pool, water).await,
        0,
        "stock must not go negative"
    );
    assert_eq!(
        count(&pool, "sale_items").await,
        1,
        "the sale is still recorded"
    );
    assert_eq!(
        count(&pool, "stock_movements").await,
        movements_before,
        "no movement for uncounted products"
    );
}

#[sqlx::test]
async fn limited_and_unlimited_products_can_share_a_sale(pool: PgPool) {
    let f = fixture(&pool, 2).await;
    let water = db::products::create(&pool, f.category_id, "Leitungswasser", 100, true)
        .await
        .unwrap();
    checkout(&pool, f.user_id, &[line(f.beer, 2), line(water, 3)], 2000)
        .await
        .unwrap();
    assert_eq!(stock_of(&pool, f.beer).await, 0);

    // the counted product is still protected
    let err = checkout(&pool, f.user_id, &[line(f.beer, 1), line(water, 1)], 2000)
        .await
        .unwrap_err();
    assert!(matches!(err, CheckoutError::Insufficient { .. }), "{err:?}");
}

#[sqlx::test]
async fn stock_cannot_be_booked_on_an_unlimited_product(pool: PgPool) {
    let f = fixture(&pool, 0).await;
    let water = db::products::create(&pool, f.category_id, "Leitungswasser", 100, true)
        .await
        .unwrap();
    let err = adjust(&pool, water, 5, REASON_RECEIPT, None, f.user_id)
        .await
        .unwrap_err();
    assert!(matches!(err, StockError::Unlimited), "{err:?}");
}

#[sqlx::test]
async fn product_can_be_switched_to_unlimited_and_back(pool: PgPool) {
    let f = fixture(&pool, 1).await;
    db::products::update(&pool, f.beer, f.category_id, "Bier", 300, true)
        .await
        .unwrap();
    assert!(
        db::products::get(&pool, f.beer)
            .await
            .unwrap()
            .unwrap()
            .unlimited
    );
    checkout(&pool, f.user_id, &[line(f.beer, 5)], 2000)
        .await
        .unwrap();

    db::products::update(&pool, f.beer, f.category_id, "Bier", 300, false)
        .await
        .unwrap();
    let err = checkout(&pool, f.user_id, &[line(f.beer, 5)], 2000)
        .await
        .unwrap_err();
    assert!(matches!(err, CheckoutError::Insufficient { .. }), "{err:?}");
}
