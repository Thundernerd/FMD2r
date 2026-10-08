//! Small helpers shared by the repositories.

use std::time::{SystemTime, UNIX_EPOCH};

/// Current time as Unix milliseconds, the timestamp unit of every table.
pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Declares a fieldless enum stored as a fixed TEXT value, with `ToSql`/`FromSql`.
macro_rules! text_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident { $( $(#[$vmeta:meta])* $variant:ident = $text:literal ),+ $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name { $( $(#[$vmeta])* $variant ),+ }

        impl $name {
            /// The TEXT value stored in the database.
            pub fn as_str(self) -> &'static str {
                match self { $($name::$variant => $text),+ }
            }
        }

        impl std::str::FromStr for $name {
            type Err = $crate::StoreError;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $($text => Ok($name::$variant),)+
                    _ => Err($crate::StoreError::InvalidColumn {
                        column: stringify!($name),
                        value: s.to_string(),
                    }),
                }
            }
        }

        impl rusqlite::ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
                Ok(self.as_str().into())
            }
        }

        impl rusqlite::types::FromSql for $name {
            fn column_result(
                value: rusqlite::types::ValueRef<'_>,
            ) -> rusqlite::types::FromSqlResult<Self> {
                value
                    .as_str()?
                    .parse()
                    .map_err(|e| rusqlite::types::FromSqlError::Other(Box::new(e)))
            }
        }
    };
}
pub(crate) use text_enum;

/// SQL expression for the `sort_order` that appends a row to the end of `table`.
pub(crate) fn next_sort_order(table: &str) -> String {
    format!("(SELECT COALESCE(MAX(sort_order), -1) + 1 FROM {table})")
}

/// Rewrites `sort_order` of `table` so `first` come first in the given order (repeats ignored)
/// and every other row keeps its relative order after them. `table` is always a constant from this
/// crate.
pub(crate) fn reorder(
    conn: &mut rusqlite::Connection,
    table: &str,
    first: impl IntoIterator<Item = i64>,
) -> crate::Result<()> {
    let mut order: Vec<i64> = Vec::new();
    for id in first {
        if !order.contains(&id) {
            order.push(id);
        }
    }
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare_cached(&format!("SELECT id FROM {table} ORDER BY sort_order"))?;
        let all = stmt.query_map([], |r| r.get::<_, i64>(0))?;
        for id in all {
            let id = id?;
            if !order.contains(&id) {
                order.push(id);
            }
        }
    }
    {
        let mut stmt =
            tx.prepare_cached(&format!("UPDATE {table} SET sort_order = ?2 WHERE id = ?1"))?;
        for (position, id) in order.iter().enumerate() {
            stmt.execute(rusqlite::params![id, position])?;
        }
    }
    tx.commit()?;
    Ok(())
}
