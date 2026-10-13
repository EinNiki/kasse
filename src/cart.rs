//! Cart logic on plain data. The cart lives in the session row and only holds product ids and
//! quantities; prices and stock are always read from the database.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CartLine {
    pub product_id: i64,
    pub quantity: i32,
}

/// Cap per product for products without a stock limit.
pub const UNLIMITED_CART_MAX: i32 = 999;

/// Adds one unit, but never more than `max_stock`.
pub fn add(lines: &mut Vec<CartLine>, product_id: i64, max_stock: i32) {
    match lines.iter_mut().find(|l| l.product_id == product_id) {
        Some(line) if line.quantity < max_stock => line.quantity += 1,
        Some(_) => {}
        None if max_stock > 0 => lines.push(CartLine {
            product_id,
            quantity: 1,
        }),
        None => {}
    }
}

/// Removes one unit; the position disappears at zero.
pub fn decrement(lines: &mut Vec<CartLine>, product_id: i64) {
    if let Some(line) = lines.iter_mut().find(|l| l.product_id == product_id) {
        line.quantity -= 1;
    }
    lines.retain(|l| l.quantity > 0);
}

pub fn remove(lines: &mut Vec<CartLine>, product_id: i64) {
    lines.retain(|l| l.product_id != product_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_respects_stock() {
        let mut c = vec![];
        add(&mut c, 1, 2);
        add(&mut c, 1, 2);
        add(&mut c, 1, 2);
        assert_eq!(
            c,
            vec![CartLine {
                product_id: 1,
                quantity: 2
            }]
        );
    }

    #[test]
    fn sold_out_product_is_not_added() {
        let mut c = vec![];
        add(&mut c, 1, 0);
        assert!(c.is_empty());
    }

    #[test]
    fn decrement_removes_at_zero() {
        let mut c = vec![
            CartLine {
                product_id: 1,
                quantity: 1,
            },
            CartLine {
                product_id: 2,
                quantity: 3,
            },
        ];
        decrement(&mut c, 1);
        decrement(&mut c, 2);
        assert_eq!(
            c,
            vec![CartLine {
                product_id: 2,
                quantity: 2
            }]
        );
    }

    #[test]
    fn remove_drops_whole_position() {
        let mut c = vec![CartLine {
            product_id: 1,
            quantity: 5,
        }];
        remove(&mut c, 1);
        assert!(c.is_empty());
    }
}
