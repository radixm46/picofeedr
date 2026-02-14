//! Feed reconciliation and CLI rendering.

mod api;
mod identity;
mod reconcile;

pub use api::FeedListResponse;
pub use api::render_feed_list;
pub use identity::feed_id_from_url;
pub use reconcile::reconcile_feeds;
