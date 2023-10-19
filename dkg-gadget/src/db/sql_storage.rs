use super::*;
use crate::{async_protocols::types::LocalKeyType, debug_logger::DebugLogger, DKGKeystore};
use dkg_primitives::{
	types::DKGError,
	utils::{decrypt_data, encrypt_data},
	SessionId,
};
use dkg_runtime_primitives::offchain::crypto::{Pair as AppPair, Public};
use sc_client_api::Backend;
use sc_keystore::LocalKeystore;
use sp_core::{offchain::OffchainStorage, Pair};
use sp_runtime::{
	generic::BlockId,
	traits::{BlakeTwo256, Block as BlockT, Header as HeaderT, UniqueSaturatedInto, Zero},
};
use sqlx::{
	query::Query,
	sqlite::{
		SqliteArguments, SqliteConnectOptions, SqlitePool, SqlitePoolOptions, SqliteQueryResult,
	},
	ConnectOptions, Error, Execute, QueryBuilder, Row, Sqlite,
};
use std::{cmp::Ordering, collections::HashSet, num::NonZeroU32, str::FromStr, sync::Arc};

// Represents the Sqlite connection options that are
/// used to establish a database connection.
#[derive(Debug)]
pub struct SqliteBackendConfig<'a> {
	pub path: &'a str,
	pub create_if_missing: bool,
	pub thread_count: u32,
	pub cache_size: u64,
}

/// Represents the backend configurations.
#[derive(Debug)]
pub enum BackendConfig<'a> {
	Sqlite(SqliteBackendConfig<'a>),
}

#[derive(Clone)]
pub struct SqlBackend {
	/// The Sqlite connection.
	pool: SqlitePool,

	/// The number of allowed operations for the Sqlite filter call.
	/// A value of `0` disables the timeout.
	num_ops_timeout: i32,
}

impl SqlBackend {
	/// Creates a new instance of the SQL backend.
	pub async fn new(
		config: BackendConfig<'_>,
		pool_size: u32,
		num_ops_timeout: Option<NonZeroU32>,
	) -> Result<Self, Error> {
		let any_pool = SqlitePoolOptions::new()
			.max_connections(pool_size)
			.connect_lazy_with(Self::connect_options(&config)?.disable_statement_logging());
		let _ = Self::create_database_if_not_exists(&any_pool).await?;
		let _ = Self::create_indexes_if_not_exist(&any_pool).await?;
		Ok(Self {
			pool: any_pool,
			num_ops_timeout: num_ops_timeout
				.map(|n| n.get())
				.unwrap_or(0)
				.try_into()
				.unwrap_or(i32::MAX),
		})
	}

	fn connect_options(config: &BackendConfig) -> Result<SqliteConnectOptions, Error> {
		match config {
			BackendConfig::Sqlite(config) => {
				log::info!(target: "frontier-sql", "📑 Connection configuration: {config:?}");
				let config = sqlx::sqlite::SqliteConnectOptions::from_str(config.path)?
					.create_if_missing(config.create_if_missing)
					// https://www.sqlite.org/pragma.html#pragma_busy_timeout
					.busy_timeout(std::time::Duration::from_secs(8))
					// 200MB, https://www.sqlite.org/pragma.html#pragma_cache_size
					.pragma("cache_size", format!("-{}", config.cache_size))
					// https://www.sqlite.org/pragma.html#pragma_analysis_limit
					.pragma("analysis_limit", "1000")
					// https://www.sqlite.org/pragma.html#pragma_threads
					.pragma("threads", config.thread_count.to_string())
					// https://www.sqlite.org/pragma.html#pragma_threads
					.pragma("temp_store", "memory")
					// https://www.sqlite.org/wal.html
					.journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
					// https://www.sqlite.org/pragma.html#pragma_synchronous
					.synchronous(sqlx::sqlite::SqliteSynchronous::Normal);
				Ok(config)
			},
		}
	}

	/// Get the underlying Sqlite pool.
	pub fn pool(&self) -> &SqlitePool {
		&self.pool
	}

	/// Create the Sqlite database if it does not already exist.
	async fn create_database_if_not_exists(pool: &SqlitePool) -> Result<SqliteQueryResult, Error> {
		sqlx::query(
			"BEGIN;
			CREATE TABLE IF NOT EXISTS dkg_keys (
				id INTEGER PRIMARY KEY,
				session_id INTEGER NOT NULL,
				local_key BLOB NOT NULL,
			);
			COMMIT;",
		)
		.execute(pool)
		.await
	}

	/// Create the Sqlite database indices if it does not already exist.
	async fn create_indexes_if_not_exist(pool: &SqlitePool) -> Result<SqliteQueryResult, Error> {
		sqlx::query(
			"BEGIN;
			CREATE INDEX IF NOT EXISTS session_id_index ON dkg_keys (
				session_id
			);
			COMMIT;",
		)
		.execute(pool)
		.await
	}

	async fn get_local_key(&self, session_id: SessionId) -> Result<Option<LocalKeyType>, DKGError> {
		log::info!(
			"{}",
			format!("Offchain Storage : Fetching local keys for session {session_id:?}")
		);
		let session_id: i64 = session_id as i64;
		match sqlx::query("SELECT local_key FROM dkg_keys WHERE session_id = ?")
			.bind(session_id)
			.fetch_optional(self.pool())
			.await
		{
			Ok(result) => {
				if let Some(row) = result {
					let local_key_json: String = row.get(0);
					let local_key: LocalKeyType = serde_json::from_str(&local_key_json).unwrap();
					return Ok(Some(local_key))
				}
				return Err(DKGError::LocalKeyNotFound)
			},
			Err(err) => {
				log::debug!(target: "dkg-gadget", "Failed retrieving key for session_id {err:?}");
				return Err(DKGError::LocalKeyNotFound)
			},
		}
	}

	async fn store_local_key(
		&self,
		session_id: SessionId,
		local_key: LocalKeyType,
	) -> Result<(), DKGError> {
		log::info!(
			"{}",
			format!(
			"Offchain Storage : Store local keys for session {session_id:?}, Key : {local_key:?}"
		)
		);
		let session_id: i64 = session_id as i64;
		let mut tx = self.pool().begin().await.map_err(|_| DKGError::StoringLocalKeyFailed)?;
		let local_key = serde_json::to_string(&local_key).unwrap();
		sqlx::query("INSERT INTO dkg_keys(session_id, local_key) VALUES (?)")
			.bind(session_id)
			.bind(local_key)
			.execute(&mut *tx)
			.await
			.map_err(|_| DKGError::StoringLocalKeyFailed)?;

		log::debug!(target: "frontier-sql", "[Metadata] Ready to commit");
		tx.commit().await.map_err(|_| DKGError::StoringLocalKeyFailed)
	}
}
