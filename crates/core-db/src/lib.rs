//! vyreon-core's database: the migrations and (later) the queries over them. Core owns every
//! table and is the only writer (docs/DECISIONS.md C-019).
pub mod api_key;
pub mod archive;
pub mod backfill;
pub mod budget;
pub mod build;
pub mod capture;
pub mod census;
pub mod daily;
pub mod deaths;
pub mod evidence;
pub mod instance;
pub mod migrate;
pub mod notice;
pub mod plain;
pub mod progress;
pub mod publish;
pub mod recompute;
pub mod refcheck;
pub mod republish;
pub mod runview;
pub mod spend;
pub mod stoprule;
pub mod tape;
pub mod universe;
pub mod watch;
pub mod work;
