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

use common_ast::ast::FormatTreeNode;
use common_catalog::plan::PartStatistics;
use common_exception::Result;
use common_functions::scalars::BUILTIN_FUNCTIONS;
use common_profile::ProfSpanSetRef;
use itertools::Itertools;

use super::AggregateExpand;
use super::AggregateFinal;
use super::AggregateFunctionDesc;
use super::AggregatePartial;
use super::EvalScalar;
use super::Exchange;
use super::Filter;
use super::HashJoin;
use super::Limit;
use super::PhysicalPlan;
use super::Project;
use super::Sort;
use super::TableScan;
use super::UnionAll;
use super::Unnest;
use crate::executor::explain::PlanStatsInfo;
use crate::executor::DistributedInsertSelect;
use crate::executor::ExchangeSink;
use crate::executor::ExchangeSource;
use crate::executor::FragmentKind;
use crate::executor::RuntimeFilterSource;
use crate::planner::MetadataRef;
use crate::planner::DUMMY_TABLE_INDEX;
use crate::BaseTableColumn;
use crate::ColumnEntry;
use crate::DerivedColumn;
use crate::TableInternalColumn;

impl PhysicalPlan {
    pub fn format(
        &self,
        metadata: MetadataRef,
        prof_span_set: ProfSpanSetRef,
    ) -> Result<FormatTreeNode<String>> {
        to_format_tree(self, &metadata, &prof_span_set)
    }
}

fn to_format_tree(
    plan: &PhysicalPlan,
    metadata: &MetadataRef,
    prof_span_set: &ProfSpanSetRef,
) -> Result<FormatTreeNode<String>> {
    match plan {
        PhysicalPlan::TableScan(plan) => table_scan_to_format_tree(plan, metadata),
        PhysicalPlan::Filter(plan) => filter_to_format_tree(plan, metadata, prof_span_set),
        PhysicalPlan::Project(plan) => project_to_format_tree(plan, metadata, prof_span_set),
        PhysicalPlan::EvalScalar(plan) => eval_scalar_to_format_tree(plan, metadata, prof_span_set),
        PhysicalPlan::AggregateExpand(plan) => {
            aggregate_expand_to_format_tree(plan, metadata, prof_span_set)
        }
        PhysicalPlan::AggregatePartial(plan) => {
            aggregate_partial_to_format_tree(plan, metadata, prof_span_set)
        }
        PhysicalPlan::AggregateFinal(plan) => {
            aggregate_final_to_format_tree(plan, metadata, prof_span_set)
        }
        PhysicalPlan::Sort(plan) => sort_to_format_tree(plan, metadata, prof_span_set),
        PhysicalPlan::Limit(plan) => limit_to_format_tree(plan, metadata, prof_span_set),
        PhysicalPlan::HashJoin(plan) => hash_join_to_format_tree(plan, metadata, prof_span_set),
        PhysicalPlan::Exchange(plan) => exchange_to_format_tree(plan, metadata, prof_span_set),
        PhysicalPlan::UnionAll(plan) => union_all_to_format_tree(plan, metadata, prof_span_set),
        PhysicalPlan::ExchangeSource(plan) => exchange_source_to_format_tree(plan),
        PhysicalPlan::ExchangeSink(plan) => {
            exchange_sink_to_format_tree(plan, metadata, prof_span_set)
        }
        PhysicalPlan::DistributedInsertSelect(plan) => {
            distributed_insert_to_format_tree(plan.as_ref(), metadata, prof_span_set)
        }
        PhysicalPlan::Unnest(plan) => unnest_to_format_tree(plan, metadata, prof_span_set),
        PhysicalPlan::RuntimeFilterSource(plan) => {
            runtime_filter_source_to_format_tree(plan, metadata, prof_span_set)
        }
    }
}

fn table_scan_to_format_tree(
    plan: &TableScan,
    metadata: &MetadataRef,
) -> Result<FormatTreeNode<String>> {
    if plan.table_index == DUMMY_TABLE_INDEX {
        return Ok(FormatTreeNode::new("DummyTableScan".to_string()));
    }
    let table = metadata.read().table(plan.table_index).clone();
    let table_name = format!("{}.{}.{}", table.catalog(), table.database(), table.name());
    let filters = plan
        .source
        .push_downs
        .as_ref()
        .and_then(|extras| {
            extras
                .filter
                .as_ref()
                .map(|expr| expr.as_expr(&BUILTIN_FUNCTIONS).sql_display())
        })
        .unwrap_or_default();

    let limit = plan
        .source
        .push_downs
        .as_ref()
        .map_or("NONE".to_string(), |extras| {
            extras
                .limit
                .map_or("NONE".to_string(), |limit| limit.to_string())
        });

    let mut children = vec![FormatTreeNode::new(format!("table: {table_name}"))];

    // Part stats.
    children.extend(part_stats_info_to_format_tree(&plan.source.statistics));
    // Push downs.
    children.push(FormatTreeNode::new(format!(
        "push downs: [filters: [{filters}], limit: {limit}]"
    )));

    let output_columns = plan.source.output_schema.fields();

    // If output_columns contains all columns of the source,
    // Then output_columns won't show in explain
    if output_columns.len() < plan.source.source_info.schema().fields().len() {
        children.push(FormatTreeNode::new(format!(
            "output columns: [{}]",
            output_columns.iter().map(|f| f.name()).join(", ")
        )));
    }

    if let Some(info) = &plan.stat_info {
        let items = plan_stats_info_to_format_tree(info);
        children.extend(items);
    }

    Ok(FormatTreeNode::with_children(
        "TableScan".to_string(),
        children,
    ))
}

fn filter_to_format_tree(
    plan: &Filter,
    metadata: &MetadataRef,
    prof_span_set: &ProfSpanSetRef,
) -> Result<FormatTreeNode<String>> {
    let filter = plan
        .predicates
        .iter()
        .map(|pred| pred.as_expr(&BUILTIN_FUNCTIONS).sql_display())
        .join(", ");
    let mut children = vec![FormatTreeNode::new(format!("filters: [{filter}]"))];

    if let Some(info) = &plan.stat_info {
        let items = plan_stats_info_to_format_tree(info);
        children.extend(items);
    }

    if let Some(prof_span) = prof_span_set.lock().unwrap().get(&plan.plan_id) {
        let process_time = prof_span.process_time / 1000 / 1000; // milliseconds
        children.push(FormatTreeNode::new(format!(
            "total process time: {process_time}ms"
        )));
    }

    children.push(to_format_tree(&plan.input, metadata, prof_span_set)?);

    Ok(FormatTreeNode::with_children(
        "Filter".to_string(),
        children,
    ))
}

fn project_to_format_tree(
    plan: &Project,
    metadata: &MetadataRef,
    prof_span_set: &ProfSpanSetRef,
) -> Result<FormatTreeNode<String>> {
    let columns = plan
        .columns
        .iter()
        .sorted()
        .map(|column| {
            format!(
                "{} (#{})",
                match metadata.read().column(*column) {
                    ColumnEntry::BaseTableColumn(BaseTableColumn { column_name, .. }) =>
                        column_name,
                    ColumnEntry::DerivedColumn(DerivedColumn { alias, .. }) => alias,
                    ColumnEntry::InternalColumn(TableInternalColumn {
                        internal_column, ..
                    }) => internal_column.column_name(),
                },
                column
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let mut children = vec![FormatTreeNode::new(format!("columns: [{columns}]"))];

    if let Some(info) = &plan.stat_info {
        let items = plan_stats_info_to_format_tree(info);
        children.extend(items);
    }

    if let Some(prof_span) = prof_span_set.lock().unwrap().get(&plan.plan_id) {
        let process_time = prof_span.process_time / 1000 / 1000; // milliseconds
        children.push(FormatTreeNode::new(format!(
            "total process time: {process_time}ms"
        )));
    }

    children.push(to_format_tree(&plan.input, metadata, prof_span_set)?);

    Ok(FormatTreeNode::with_children(
        "Project".to_string(),
        children,
    ))
}

fn eval_scalar_to_format_tree(
    plan: &EvalScalar,
    metadata: &MetadataRef,
    prof_span_set: &ProfSpanSetRef,
) -> Result<FormatTreeNode<String>> {
    let scalars = plan
        .exprs
        .iter()
        .map(|(expr, _)| expr.as_expr(&BUILTIN_FUNCTIONS).sql_display())
        .collect::<Vec<_>>()
        .join(", ");
    let mut children = vec![FormatTreeNode::new(format!("expressions: [{scalars}]"))];

    if let Some(info) = &plan.stat_info {
        let items = plan_stats_info_to_format_tree(info);
        children.extend(items);
    }

    if let Some(prof_span) = prof_span_set.lock().unwrap().get(&plan.plan_id) {
        let process_time = prof_span.process_time / 1000 / 1000; // milliseconds
        children.push(FormatTreeNode::new(format!(
            "total process time: {process_time}ms"
        )));
    }

    children.push(to_format_tree(&plan.input, metadata, prof_span_set)?);

    Ok(FormatTreeNode::with_children(
        "EvalScalar".to_string(),
        children,
    ))
}

pub fn pretty_display_agg_desc(desc: &AggregateFunctionDesc, metadata: &MetadataRef) -> String {
    format!(
        "{}({})",
        desc.sig.name,
        desc.arg_indices
            .iter()
            .map(|&index| {
                let column = metadata.read().column(index).clone();
                match column {
                    ColumnEntry::BaseTableColumn(BaseTableColumn { column_name, .. }) => {
                        column_name
                    }
                    ColumnEntry::DerivedColumn(DerivedColumn { alias, .. }) => alias,
                    ColumnEntry::InternalColumn(TableInternalColumn {
                        internal_column, ..
                    }) => internal_column.column_name().to_string(),
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn aggregate_expand_to_format_tree(
    plan: &AggregateExpand,
    metadata: &MetadataRef,
    prof_span_set: &ProfSpanSetRef,
) -> Result<FormatTreeNode<String>> {
    let sets = plan
        .grouping_sets
        .iter()
        .map(|set| {
            set.iter()
                .map(|column| {
                    let column = metadata.read().column(*column).clone();
                    match column {
                        ColumnEntry::BaseTableColumn(BaseTableColumn { column_name, .. }) => {
                            column_name
                        }
                        ColumnEntry::DerivedColumn(DerivedColumn { alias, .. }) => alias,
                        ColumnEntry::InternalColumn(TableInternalColumn {
                            internal_column,
                            ..
                        }) => internal_column.column_name().to_string(),
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .map(|s| format!("({})", s))
        .collect::<Vec<_>>()
        .join(", ");

    let mut children = vec![FormatTreeNode::new(format!("grouping sets: [{sets}]"))];

    if let Some(info) = &plan.stat_info {
        let items = plan_stats_info_to_format_tree(info);
        children.extend(items);
    }

    if let Some(prof_span) = prof_span_set.lock().unwrap().get(&plan.plan_id) {
        let process_time = prof_span.process_time / 1000 / 1000; // milliseconds
        children.push(FormatTreeNode::new(format!(
            "total process time: {process_time}ms"
        )));
    }

    children.push(to_format_tree(&plan.input, metadata, prof_span_set)?);

    Ok(FormatTreeNode::with_children(
        "AggregateExpand".to_string(),
        children,
    ))
}

fn aggregate_partial_to_format_tree(
    plan: &AggregatePartial,
    metadata: &MetadataRef,
    prof_span_set: &ProfSpanSetRef,
) -> Result<FormatTreeNode<String>> {
    let group_by = plan
        .group_by
        .iter()
        .map(|column| {
            let column = metadata.read().column(*column).clone();
            let name = match column {
                ColumnEntry::BaseTableColumn(BaseTableColumn { column_name, .. }) => column_name,
                ColumnEntry::DerivedColumn(DerivedColumn { alias, .. }) => alias,
                ColumnEntry::InternalColumn(TableInternalColumn {
                    internal_column, ..
                }) => internal_column.column_name().to_string(),
            };
            Ok(name)
        })
        .collect::<Result<Vec<_>>>()?
        .join(", ");
    let agg_funcs = plan
        .agg_funcs
        .iter()
        .map(|agg| pretty_display_agg_desc(agg, metadata))
        .collect::<Vec<_>>()
        .join(", ");

    let mut children = vec![
        FormatTreeNode::new(format!("group by: [{group_by}]")),
        FormatTreeNode::new(format!("aggregate functions: [{agg_funcs}]")),
    ];

    if let Some(info) = &plan.stat_info {
        let items = plan_stats_info_to_format_tree(info);
        children.extend(items);
    }

    if let Some(prof_span) = prof_span_set.lock().unwrap().get(&plan.plan_id) {
        let process_time = prof_span.process_time / 1000 / 1000; // milliseconds
        children.push(FormatTreeNode::new(format!(
            "total process time: {process_time}ms"
        )));
    }

    children.push(to_format_tree(&plan.input, metadata, prof_span_set)?);

    Ok(FormatTreeNode::with_children(
        "AggregatePartial".to_string(),
        children,
    ))
}

fn aggregate_final_to_format_tree(
    plan: &AggregateFinal,
    metadata: &MetadataRef,
    prof_span_set: &ProfSpanSetRef,
) -> Result<FormatTreeNode<String>> {
    let group_by = plan
        .group_by
        .iter()
        .map(|column| {
            let column = metadata.read().column(*column).clone();
            let name = match column {
                ColumnEntry::BaseTableColumn(BaseTableColumn { column_name, .. }) => column_name,
                ColumnEntry::DerivedColumn(DerivedColumn { alias, .. }) => alias,
                ColumnEntry::InternalColumn(TableInternalColumn {
                    internal_column, ..
                }) => internal_column.column_name().to_string(),
            };
            Ok(name)
        })
        .collect::<Result<Vec<_>>>()?
        .join(", ");

    let agg_funcs = plan
        .agg_funcs
        .iter()
        .map(|agg| pretty_display_agg_desc(agg, metadata))
        .collect::<Vec<_>>()
        .join(", ");

    let mut children = vec![
        FormatTreeNode::new(format!("group by: [{group_by}]")),
        FormatTreeNode::new(format!("aggregate functions: [{agg_funcs}]")),
    ];

    if let Some(limit) = &plan.limit {
        let items = FormatTreeNode::new(format!("limit: {limit}"));
        children.push(items);
    }

    if let Some(info) = &plan.stat_info {
        let items = plan_stats_info_to_format_tree(info);
        children.extend(items);
    }

    if let Some(prof_span) = prof_span_set.lock().unwrap().get(&plan.plan_id) {
        let process_time = prof_span.process_time / 1000 / 1000; // milliseconds
        children.push(FormatTreeNode::new(format!(
            "total process time: {process_time}ms"
        )));
    }

    children.push(to_format_tree(&plan.input, metadata, prof_span_set)?);

    Ok(FormatTreeNode::with_children(
        "AggregateFinal".to_string(),
        children,
    ))
}

fn sort_to_format_tree(
    plan: &Sort,
    metadata: &MetadataRef,
    prof_span_set: &ProfSpanSetRef,
) -> Result<FormatTreeNode<String>> {
    let sort_keys = plan
        .order_by
        .iter()
        .map(|sort_key| {
            let index = sort_key.order_by;
            let column = metadata.read().column(index).clone();
            Ok(format!(
                "{} {} {}",
                match column {
                    ColumnEntry::BaseTableColumn(BaseTableColumn { column_name, .. }) =>
                        column_name,
                    ColumnEntry::DerivedColumn(DerivedColumn { alias, .. }) => alias,
                    ColumnEntry::InternalColumn(TableInternalColumn {
                        internal_column, ..
                    }) => {
                        internal_column.column_name().to_string()
                    }
                },
                if sort_key.asc { "ASC" } else { "DESC" },
                if sort_key.nulls_first {
                    "NULLS FIRST"
                } else {
                    "NULLS LAST"
                }
            ))
        })
        .collect::<Result<Vec<_>>>()?
        .join(", ");

    let mut children = vec![FormatTreeNode::new(format!("sort keys: [{sort_keys}]"))];

    if let Some(info) = &plan.stat_info {
        let items = plan_stats_info_to_format_tree(info);
        children.extend(items);
    }

    if let Some(prof_span) = prof_span_set.lock().unwrap().get(&plan.plan_id) {
        let process_time = prof_span.process_time / 1000 / 1000; // milliseconds
        children.push(FormatTreeNode::new(format!(
            "total process time: {process_time}ms"
        )));
    }

    children.push(to_format_tree(&plan.input, metadata, prof_span_set)?);

    Ok(FormatTreeNode::with_children("Sort".to_string(), children))
}

fn limit_to_format_tree(
    plan: &Limit,
    metadata: &MetadataRef,
    prof_span_set: &ProfSpanSetRef,
) -> Result<FormatTreeNode<String>> {
    let mut children = vec![
        FormatTreeNode::new(format!(
            "limit: {}",
            plan.limit
                .map_or("NONE".to_string(), |limit| limit.to_string())
        )),
        FormatTreeNode::new(format!("offset: {}", plan.offset)),
    ];

    if let Some(info) = &plan.stat_info {
        let items = plan_stats_info_to_format_tree(info);
        children.extend(items);
    }

    if let Some(prof_span) = prof_span_set.lock().unwrap().get(&plan.plan_id) {
        let process_time = prof_span.process_time / 1000 / 1000; // milliseconds
        children.push(FormatTreeNode::new(format!(
            "total process time: {process_time}ms"
        )));
    }

    children.push(to_format_tree(&plan.input, metadata, prof_span_set)?);

    Ok(FormatTreeNode::with_children("Limit".to_string(), children))
}

fn hash_join_to_format_tree(
    plan: &HashJoin,
    metadata: &MetadataRef,
    prof_span_set: &ProfSpanSetRef,
) -> Result<FormatTreeNode<String>> {
    let build_keys = plan
        .build_keys
        .iter()
        .map(|scalar| scalar.as_expr(&BUILTIN_FUNCTIONS).sql_display())
        .collect::<Vec<_>>()
        .join(", ");
    let probe_keys = plan
        .probe_keys
        .iter()
        .map(|scalar| scalar.as_expr(&BUILTIN_FUNCTIONS).sql_display())
        .collect::<Vec<_>>()
        .join(", ");
    let filters = plan
        .non_equi_conditions
        .iter()
        .map(|filter| filter.as_expr(&BUILTIN_FUNCTIONS).sql_display())
        .collect::<Vec<_>>()
        .join(", ");

    let mut build_child = to_format_tree(&plan.build, metadata, prof_span_set)?;
    let mut probe_child = to_format_tree(&plan.probe, metadata, prof_span_set)?;

    build_child.payload = format!("{}(Build)", build_child.payload);
    probe_child.payload = format!("{}(Probe)", probe_child.payload);

    let mut children = vec![
        FormatTreeNode::new(format!("join type: {}", plan.join_type)),
        FormatTreeNode::new(format!("build keys: [{build_keys}]")),
        FormatTreeNode::new(format!("probe keys: [{probe_keys}]")),
        FormatTreeNode::new(format!("filters: [{filters}]")),
    ];

    if let Some(info) = &plan.stat_info {
        let items = plan_stats_info_to_format_tree(info);
        children.extend(items);
    }

    if let Some(prof_span) = prof_span_set.lock().unwrap().get(&plan.plan_id) {
        let process_time = prof_span.process_time / 1000 / 1000; // milliseconds
        children.push(FormatTreeNode::new(format!(
            "total process time: {process_time}ms"
        )));
    }

    children.push(build_child);
    children.push(probe_child);

    Ok(FormatTreeNode::with_children(
        "HashJoin".to_string(),
        children,
    ))
}

fn exchange_to_format_tree(
    plan: &Exchange,
    metadata: &MetadataRef,
    prof_span_set: &ProfSpanSetRef,
) -> Result<FormatTreeNode<String>> {
    Ok(FormatTreeNode::with_children("Exchange".to_string(), vec![
        FormatTreeNode::new(format!("exchange type: {}", match plan.kind {
            FragmentKind::Init => "Init-Partition".to_string(),
            FragmentKind::Normal => format!(
                "Hash({})",
                plan.keys
                    .iter()
                    .map(|key| { key.as_expr(&BUILTIN_FUNCTIONS).sql_display() })
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            FragmentKind::Expansive => "Broadcast".to_string(),
            FragmentKind::Merge => "Merge".to_string(),
        })),
        to_format_tree(&plan.input, metadata, prof_span_set)?,
    ]))
}

fn union_all_to_format_tree(
    plan: &UnionAll,
    metadata: &MetadataRef,
    prof_span_set: &ProfSpanSetRef,
) -> Result<FormatTreeNode<String>> {
    let mut children = vec![];

    if let Some(info) = &plan.stat_info {
        let items = plan_stats_info_to_format_tree(info);
        children.extend(items);
    }

    if let Some(prof_span) = prof_span_set.lock().unwrap().get(&plan.plan_id) {
        let process_time = prof_span.process_time / 1000 / 1000; // milliseconds
        children.push(FormatTreeNode::new(format!(
            "total process time: {process_time}ms"
        )));
    }

    children.extend(vec![
        to_format_tree(&plan.left, metadata, prof_span_set)?,
        to_format_tree(&plan.right, metadata, prof_span_set)?,
    ]);

    Ok(FormatTreeNode::with_children(
        "UnionAll".to_string(),
        children,
    ))
}

fn part_stats_info_to_format_tree(info: &PartStatistics) -> Vec<FormatTreeNode<String>> {
    let mut items = vec![
        FormatTreeNode::new(format!("read rows: {}", info.read_rows)),
        FormatTreeNode::new(format!("read bytes: {}", info.read_bytes)),
        FormatTreeNode::new(format!("partitions total: {}", info.partitions_total)),
        FormatTreeNode::new(format!("partitions scanned: {}", info.partitions_scanned)),
    ];

    if info.pruning_stats.segments_range_pruning_before > 0 {
        items.push(FormatTreeNode::new(format!(
            "pruning stats: [segments: <range pruning: {} to {}>, blocks: <range pruning: {} to {}, bloom pruning: {} to {}>]",
            info.pruning_stats.segments_range_pruning_before,
            info.pruning_stats.segments_range_pruning_after,
            info.pruning_stats.blocks_range_pruning_before,
            info.pruning_stats.blocks_range_pruning_after,
            info.pruning_stats.blocks_bloom_pruning_before,
            info.pruning_stats.blocks_bloom_pruning_after,
        )))
    }

    items
}

fn plan_stats_info_to_format_tree(info: &PlanStatsInfo) -> Vec<FormatTreeNode<String>> {
    vec![FormatTreeNode::new(format!(
        "estimated rows: {0:.2}",
        info.estimated_rows
    ))]
}

fn exchange_source_to_format_tree(plan: &ExchangeSource) -> Result<FormatTreeNode<String>> {
    let mut children = vec![];

    children.push(FormatTreeNode::new(format!(
        "source fragment: [{}]",
        plan.source_fragment_id
    )));

    Ok(FormatTreeNode::with_children(
        "ExchangeSource".to_string(),
        children,
    ))
}

fn exchange_sink_to_format_tree(
    plan: &ExchangeSink,
    metadata: &MetadataRef,
    prof_span_set: &ProfSpanSetRef,
) -> Result<FormatTreeNode<String>> {
    let mut children = vec![];

    children.push(FormatTreeNode::new(format!(
        "destination fragment: [{}]",
        plan.destination_fragment_id
    )));

    children.push(to_format_tree(&plan.input, metadata, prof_span_set)?);

    Ok(FormatTreeNode::with_children(
        "ExchangeSink".to_string(),
        children,
    ))
}

fn distributed_insert_to_format_tree(
    plan: &DistributedInsertSelect,
    metadata: &MetadataRef,
    prof_span_set: &ProfSpanSetRef,
) -> Result<FormatTreeNode<String>> {
    let children = vec![to_format_tree(&plan.input, metadata, prof_span_set)?];

    Ok(FormatTreeNode::with_children(
        "DistributedInsertSelect".to_string(),
        children,
    ))
}

fn unnest_to_format_tree(
    plan: &Unnest,
    metadata: &MetadataRef,
    prof_span_set: &ProfSpanSetRef,
) -> Result<FormatTreeNode<String>> {
    let children = vec![to_format_tree(&plan.input, metadata, prof_span_set)?];

    Ok(FormatTreeNode::with_children(
        "Unnest".to_string(),
        children,
    ))
}

fn runtime_filter_source_to_format_tree(
    plan: &RuntimeFilterSource,
    metadata: &MetadataRef,
    prof_span_set: &ProfSpanSetRef,
) -> Result<FormatTreeNode<String>> {
    let children = vec![
        to_format_tree(&plan.left_side, metadata, prof_span_set)?,
        to_format_tree(&plan.right_side, metadata, prof_span_set)?,
    ];
    Ok(FormatTreeNode::with_children(
        "RuntimeFilterSource".to_string(),
        children,
    ))
}
