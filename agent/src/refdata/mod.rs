//! Reference data: parsing the official DE drop tables and caching them.

pub mod model;
pub mod naming;
pub mod parse;
pub mod query;
pub mod store;

pub use model::{Drop, Era, RefinementTier, Relic};
pub use parse::{farmable_relics, parse_drop_data, parse_relics};
pub use query::{relic_view, resolve_reward, DropView, RelicView, RewardView};
