// Copyright 2022 Datafuse Labs.
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

use std::sync::Arc;

use common_ast::ast::InsertSource;
use common_ast::ast::InsertStmt;
use common_ast::ast::Statement;
use common_exception::Result;
use common_expression::TableSchemaRefExt;
use common_meta_app::principal::FileFormatOptions;

use crate::binder::Binder;
use crate::normalize_identifier;
use crate::optimizer::optimize;
use crate::optimizer::OptimizerConfig;
use crate::optimizer::OptimizerContext;
use crate::plans::Insert;
use crate::plans::InsertInputSource;
use crate::plans::Plan;
use crate::BindContext;

impl Binder {
    pub(in crate::planner::binder) async fn bind_insert(
        &mut self,
        bind_context: &BindContext,
        stmt: &InsertStmt,
    ) -> Result<Plan> {
        let InsertStmt {
            catalog,
            database,
            table,
            columns,
            source,
            overwrite,
        } = stmt;
        let (catalog_name, database_name, table_name) =
            self.normalize_object_identifier_triple(catalog, database, table);
        let table = self
            .ctx
            .get_table(&catalog_name, &database_name, &table_name)
            .await?;
        let table_id = table.get_id();

        let schema = if columns.is_empty() {
            table.schema()
        } else {
            let schema = table.schema();
            let fields = columns
                .iter()
                .map(|ident| {
                    schema
                        .field_with_name(
                            &normalize_identifier(ident, &self.name_resolution_ctx).name,
                        )
                        .map(|v| v.clone())
                })
                .collect::<Result<Vec<_>>>()?;
            TableSchemaRefExt::create(fields)
        };

        let input_source: Result<InsertInputSource> = match source.clone() {
            InsertSource::Streaming {
                format,
                rest_str,
                start,
            } => {
                if format.to_uppercase() == "VALUES" {
                    let data = rest_str.trim_end_matches(';').trim_start().to_owned();
                    Ok(InsertInputSource::Values(data))
                } else {
                    Ok(InsertInputSource::StreamingWithFormat(format, start, None))
                }
            }
            InsertSource::StreamingV2 { settings, start } => {
                let opts = FileFormatOptions::from_map(&settings)?;
                Ok(InsertInputSource::StreamingWithFileFormat(
                    opts, start, None,
                ))
            }
            InsertSource::Values { rest_str } => {
                let values_str = rest_str.trim_end_matches(';').trim_start().to_owned();
                match self.ctx.get_stage_attachment() {
                    Some(mut attachment) => {
                        attachment.values_str = values_str;
                        Ok(InsertInputSource::Stage(Arc::new(attachment)))
                    }
                    None => Ok(InsertInputSource::Values(values_str)),
                }
            }
            InsertSource::Select { query } => {
                let statement = Statement::Query(query);
                let select_plan = self.bind_statement(bind_context, &statement).await?;
                let opt_ctx = Arc::new(OptimizerContext::new(OptimizerConfig {
                    enable_distributed_optimization: !self.ctx.get_cluster().is_empty(),
                }));
                let optimized_plan = optimize(self.ctx.clone(), opt_ctx, select_plan)?;
                Ok(InsertInputSource::SelectPlan(Box::new(optimized_plan)))
            }
        };

        let plan = Insert {
            catalog: catalog_name.to_string(),
            database: database_name.to_string(),
            table: table_name,
            table_id,
            schema,
            overwrite: *overwrite,
            source: input_source?,
        };

        Ok(Plan::Insert(Box::new(plan)))
    }
}
