// Copyright 2021 Datafuse Labs.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::collections::BTreeMap;
use std::sync::Arc;

use common_exception::ErrorCode;
use common_exception::Result;
use common_meta_app::schema::CreateTableReq;
use common_meta_app::schema::DatabaseInfo;
use common_meta_app::schema::DropTableByIdReq;
use common_meta_app::schema::DropTableReply;
use common_meta_app::schema::GetTableCopiedFileReply;
use common_meta_app::schema::GetTableCopiedFileReq;
use common_meta_app::schema::RenameTableReply;
use common_meta_app::schema::RenameTableReq;
use common_meta_app::schema::TableInfo;
use common_meta_app::schema::TruncateTableReply;
use common_meta_app::schema::TruncateTableReq;
use common_meta_app::schema::UndropTableReply;
use common_meta_app::schema::UndropTableReq;
use common_meta_app::schema::UpdateTableMetaReply;
use common_meta_app::schema::UpdateTableMetaReq;
use common_meta_app::schema::UpsertTableCopiedFileReply;
use common_meta_app::schema::UpsertTableCopiedFileReq;
use common_meta_app::schema::UpsertTableOptionReply;
use common_meta_app::schema::UpsertTableOptionReq;
use dyn_clone::DynClone;

use crate::table::Table;

#[async_trait::async_trait]
pub trait Database: DynClone + Sync + Send {
    /// Database name.
    fn name(&self) -> &str;

    fn engine(&self) -> &str {
        self.get_db_info().engine()
    }

    fn engine_options(&self) -> &BTreeMap<String, String> {
        &self.get_db_info().meta.engine_options
    }

    fn options(&self) -> &BTreeMap<String, String> {
        &self.get_db_info().meta.options
    }

    fn get_db_info(&self) -> &DatabaseInfo;

    fn get_tenant(&self) -> &String {
        &self.get_db_info().name_ident.tenant
    }

    fn get_db_name(&self) -> &String {
        &self.get_db_info().name_ident.db_name
    }

    // Initial a database.
    async fn init_database(&self, _tenant: &str) -> Result<()> {
        Ok(())
    }

    // Build a `Arc<dyn Table>` from `TableInfo`.
    fn get_table_by_info(&self, _table_info: &TableInfo) -> Result<Arc<dyn Table>> {
        Err(ErrorCode::Unimplemented(format!(
            "UnImplement get_table_by_info in {} Database",
            self.name()
        )))
    }

    // Get one table by db and table name.
    async fn get_table(&self, _table_name: &str) -> Result<Arc<dyn Table>> {
        Err(ErrorCode::Unimplemented(format!(
            "UnImplement get_table in {} Database",
            self.name()
        )))
    }

    async fn list_tables(&self) -> Result<Vec<Arc<dyn Table>>> {
        Err(ErrorCode::Unimplemented(format!(
            "UnImplement list_tables in {} Database",
            self.name()
        )))
    }

    async fn list_tables_history(&self) -> Result<Vec<Arc<dyn Table>>> {
        Err(ErrorCode::Unimplemented(format!(
            "UnImplement list_tables_history in {} Database",
            self.name()
        )))
    }

    async fn create_table(&self, _req: CreateTableReq) -> Result<()> {
        Err(ErrorCode::Unimplemented(format!(
            "UnImplement create_table in {} Database",
            self.name()
        )))
    }

    async fn drop_table_by_id(&self, _req: DropTableByIdReq) -> Result<DropTableReply> {
        Err(ErrorCode::Unimplemented(format!(
            "UnImplement drop_table_by_id in {} Database",
            self.name()
        )))
    }

    async fn undrop_table(&self, _req: UndropTableReq) -> Result<UndropTableReply> {
        Err(ErrorCode::Unimplemented(format!(
            "UnImplement undrop_table in {} Database",
            self.name()
        )))
    }

    async fn rename_table(&self, _req: RenameTableReq) -> Result<RenameTableReply> {
        Err(ErrorCode::Unimplemented(format!(
            "UnImplement rename_table in {} Database",
            self.name()
        )))
    }

    async fn upsert_table_option(
        &self,
        _req: UpsertTableOptionReq,
    ) -> Result<UpsertTableOptionReply> {
        Err(ErrorCode::Unimplemented(format!(
            "UnImplement upsert_table_option in {} Database",
            self.name()
        )))
    }

    async fn update_table_meta(&self, _req: UpdateTableMetaReq) -> Result<UpdateTableMetaReply> {
        Err(ErrorCode::Unimplemented(format!(
            "UnImplement update_table_meta in {} Database",
            self.name()
        )))
    }

    async fn get_table_copied_file_info(
        &self,
        _req: GetTableCopiedFileReq,
    ) -> Result<GetTableCopiedFileReply> {
        Err(ErrorCode::Unimplemented(format!(
            "UnImplement get_table_copied_file_info in {} Database",
            self.name()
        )))
    }

    async fn upsert_table_copied_file_info(
        &self,
        _req: UpsertTableCopiedFileReq,
    ) -> Result<UpsertTableCopiedFileReply> {
        Err(ErrorCode::Unimplemented(format!(
            "UnImplement upsert_table_copied_file_info in {} Database",
            self.name()
        )))
    }

    async fn truncate_table(&self, _req: TruncateTableReq) -> Result<TruncateTableReply> {
        Err(ErrorCode::Unimplemented(format!(
            "UnImplement truncate_table in {} Database",
            self.name()
        )))
    }
}
