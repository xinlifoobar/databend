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

use std::fmt::Display;

use common_ast::ast::FormatTreeNode;
use itertools::Itertools;

use crate::optimizer::SExpr;
use crate::plans::Aggregate;
use crate::plans::AggregateMode;
use crate::plans::AndExpr;
use crate::plans::ComparisonExpr;
use crate::plans::ComparisonOp;
use crate::plans::EvalScalar;
use crate::plans::Exchange;
use crate::plans::Filter;
use crate::plans::Join;
use crate::plans::JoinType;
use crate::plans::Limit;
use crate::plans::RelOperator;
use crate::plans::ScalarExpr;
use crate::plans::Scan;
use crate::plans::Sort;
use crate::BaseTableColumn;
use crate::ColumnEntry;
use crate::DerivedColumn;
use crate::MetadataRef;
use crate::TableInternalColumn;

#[derive(Clone)]
pub enum FormatContext {
    RelOp {
        metadata: MetadataRef,
        rel_operator: Box<RelOperator>,
    },
    Text(String),
}

impl SExpr {
    pub fn to_format_tree(&self, metadata: &MetadataRef) -> FormatTreeNode<FormatContext> {
        let children: Vec<FormatTreeNode<FormatContext>> = self
            .children()
            .iter()
            .map(|child| child.to_format_tree(metadata))
            .collect();

        to_format_tree(self.plan().clone(), metadata.clone(), children)
    }
}

impl Display for FormatContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RelOp {
                metadata,
                rel_operator,
            } => match rel_operator.as_ref() {
                RelOperator::Scan(_) => write!(f, "LogicalGet"),
                RelOperator::Join(op) => format_join(f, metadata, op),
                RelOperator::EvalScalar(_) => write!(f, "EvalScalar"),
                RelOperator::Filter(_) => write!(f, "Filter"),
                RelOperator::Aggregate(op) => format_aggregate(f, metadata, op),
                RelOperator::Sort(_) => write!(f, "Sort"),
                RelOperator::Limit(_) => write!(f, "Limit"),
                RelOperator::Exchange(op) => format_exchange(f, metadata, op),
                RelOperator::UnionAll(_) => write!(f, "Union"),
                RelOperator::Pattern(_) => write!(f, "Pattern"),
                RelOperator::DummyTableScan(_) => write!(f, "DummyTableScan"),
                RelOperator::RuntimeFilterSource(_) => write!(f, "RuntimeFilterSource"),
                RelOperator::Window(_) => write!(f, "WindowFunc"),
            },
            Self::Text(text) => write!(f, "{}", text),
        }
    }
}

pub fn format_scalar(_metadata: &MetadataRef, scalar: &ScalarExpr) -> String {
    match scalar {
        ScalarExpr::BoundColumnRef(column_ref) => {
            if let Some(table_name) = &column_ref.column.table_name {
                format!(
                    "{}.{} (#{})",
                    table_name, column_ref.column.column_name, column_ref.column.index
                )
            } else {
                format!(
                    "{} (#{})",
                    column_ref.column.column_name, column_ref.column.index
                )
            }
        }
        ScalarExpr::BoundInternalColumnRef(column_ref) => {
            if let Some(table_name) = &column_ref.column.table_name {
                format!(
                    "{}.{} (#{})",
                    table_name,
                    column_ref.column.internal_column.column_name(),
                    column_ref.column.index
                )
            } else {
                format!(
                    "{} (#{})",
                    column_ref.column.internal_column.column_name(),
                    column_ref.column.index
                )
            }
        }
        ScalarExpr::ConstantExpr(constant) => constant.value.to_string(),
        ScalarExpr::AndExpr(and) => format!(
            "({}) AND ({})",
            format_scalar(_metadata, &and.left),
            format_scalar(_metadata, &and.right)
        ),
        ScalarExpr::OrExpr(or) => format!(
            "({}) OR ({})",
            format_scalar(_metadata, &or.left),
            format_scalar(_metadata, &or.right)
        ),
        ScalarExpr::NotExpr(not) => format!("NOT ({})", format_scalar(_metadata, &not.argument),),
        ScalarExpr::ComparisonExpr(comp) => format!(
            "{} {} {}",
            format_scalar(_metadata, &comp.left),
            comp.op.to_func_name(),
            format_scalar(_metadata, &comp.right)
        ),
        ScalarExpr::WindowFunction(win) => win.agg_func.display_name.clone(),
        ScalarExpr::AggregateFunction(agg) => agg.display_name.clone(),
        ScalarExpr::FunctionCall(func) => {
            format!(
                "{}({})",
                &func.func_name,
                func.arguments
                    .iter()
                    .map(|arg| { format_scalar(_metadata, arg) })
                    .collect::<Vec<String>>()
                    .join(", ")
            )
        }
        ScalarExpr::CastExpr(cast) => {
            format!(
                "CAST({} AS {})",
                format_scalar(_metadata, &cast.argument),
                cast.target_type
            )
        }
        ScalarExpr::Unnest(unnest) => {
            format!("UNNEST({})", format_scalar(_metadata, &unnest.argument),)
        }
        ScalarExpr::SubqueryExpr(_) => "SUBQUERY".to_string(),
    }
}

pub fn format_join(
    f: &mut std::fmt::Formatter<'_>,
    _metadata: &MetadataRef,
    op: &Join,
) -> std::fmt::Result {
    match op.join_type {
        JoinType::Cross => {
            write!(f, "CrossJoin")
        }
        _ => {
            write!(f, "HashJoin: {}", &op.join_type)
        }
    }
}

pub fn format_aggregate(
    f: &mut std::fmt::Formatter<'_>,
    _metadata: &MetadataRef,
    op: &Aggregate,
) -> std::fmt::Result {
    write!(f, "Aggregate({})", match &op.mode {
        AggregateMode::Partial => "Partial",
        AggregateMode::Final => "Final",
        AggregateMode::Initial => "Initial",
    })
}

pub fn format_exchange(
    f: &mut std::fmt::Formatter<'_>,
    _metadata: &MetadataRef,
    op: &Exchange,
) -> std::fmt::Result {
    match op {
        Exchange::Random => {
            write!(f, "Exchange(Random)")
        }
        Exchange::Hash(_) => {
            write!(f, "Exchange(Hash)")
        }
        Exchange::Broadcast => {
            write!(f, "Exchange(Broadcast)")
        }
        Exchange::Merge => {
            write!(f, "Exchange(Merge)")
        }
    }
}

/// Build `FormatTreeNode` for a `RelOperator`, which may returns a tree structure instead of
/// a single node.
fn to_format_tree(
    rel_operator: RelOperator,
    metadata: MetadataRef,
    children: Vec<FormatTreeNode<FormatContext>>,
) -> FormatTreeNode<FormatContext> {
    match &rel_operator {
        RelOperator::Join(op) => logical_join_to_format_tree(op, metadata, children),
        RelOperator::Scan(op) => logical_get_to_format_tree(op, metadata, children),
        RelOperator::EvalScalar(op) => eval_scalar_to_format_tree(op, metadata, children),
        RelOperator::Filter(op) => filter_to_format_tree(op, metadata, children),
        RelOperator::Aggregate(op) => aggregate_to_format_tree(op, metadata, children),
        RelOperator::Sort(op) => sort_to_format_tree(op, metadata, children),
        RelOperator::Limit(op) => limit_to_format_tree(op, metadata, children),
        RelOperator::Exchange(op) => exchange_to_format_tree(op, metadata, children),

        _ => FormatTreeNode::with_children(
            FormatContext::RelOp {
                metadata,
                rel_operator: Box::new(rel_operator),
            },
            children,
        ),
    }
}

#[allow(unused)]
fn scan_to_format_tree(
    op: &Scan,
    metadata: MetadataRef,
    children: Vec<FormatTreeNode<FormatContext>>,
) -> FormatTreeNode<FormatContext> {
    let table = metadata.read().table(op.table_index).clone();
    FormatTreeNode::with_children(
        FormatContext::RelOp {
            metadata: metadata.clone(),
            rel_operator: Box::new(op.clone().into()),
        },
        vec![
            vec![
                FormatTreeNode::new(FormatContext::Text(format!(
                    "table: {}.{}.{}",
                    table.catalog(),
                    table.database(),
                    table.name(),
                ))),
                FormatTreeNode::new(FormatContext::Text(format!(
                    "filters: [{}]",
                    op.push_down_predicates.as_ref().map_or_else(
                        || "".to_string(),
                        |predicates| {
                            predicates
                                .iter()
                                .map(|pred| format_scalar(&metadata, pred))
                                .join(", ")
                        },
                    ),
                ))),
                FormatTreeNode::new(FormatContext::Text(format!(
                    "order by: [{}]",
                    op.order_by.as_ref().map_or_else(
                        || "".to_string(),
                        |items| items
                            .iter()
                            .map(|item| format!(
                                "{} (#{}) {}",
                                match metadata.read().column(item.index) {
                                    ColumnEntry::BaseTableColumn(BaseTableColumn {
                                        column_name,
                                        ..
                                    }) => column_name,
                                    ColumnEntry::DerivedColumn(DerivedColumn { alias, .. }) =>
                                        alias,
                                    ColumnEntry::InternalColumn(TableInternalColumn {
                                        internal_column,
                                        ..
                                    }) => internal_column.column_name(),
                                },
                                item.index,
                                if item.asc { "ASC" } else { "DESC" }
                            ))
                            .collect::<Vec<String>>()
                            .join(", "),
                    ),
                ))),
                FormatTreeNode::new(FormatContext::Text(format!(
                    "limit: {}",
                    op.limit.map_or("NONE".to_string(), |l| l.to_string())
                ))),
            ],
            children,
        ]
        .concat(),
    )
}

fn logical_get_to_format_tree(
    op: &Scan,
    metadata: MetadataRef,
    children: Vec<FormatTreeNode<FormatContext>>,
) -> FormatTreeNode<FormatContext> {
    let table = metadata.read().table(op.table_index).clone();
    FormatTreeNode::with_children(
        FormatContext::RelOp {
            metadata: metadata.clone(),
            rel_operator: Box::new(op.clone().into()),
        },
        vec![
            vec![
                FormatTreeNode::new(FormatContext::Text(format!(
                    "table: {}.{}.{}",
                    table.catalog(),
                    table.database(),
                    table.name(),
                ))),
                FormatTreeNode::new(FormatContext::Text(format!(
                    "filters: [{}]",
                    op.push_down_predicates.as_ref().map_or_else(
                        || "".to_string(),
                        |predicates| {
                            predicates
                                .iter()
                                .map(|pred| format_scalar(&metadata, pred))
                                .join(", ")
                        },
                    ),
                ))),
                FormatTreeNode::new(FormatContext::Text(format!(
                    "order by: [{}]",
                    op.order_by.as_ref().map_or_else(
                        || "".to_string(),
                        |items| items
                            .iter()
                            .map(|item| format!(
                                "{} (#{}) {}",
                                match metadata.read().column(item.index) {
                                    ColumnEntry::BaseTableColumn(BaseTableColumn {
                                        column_name,
                                        ..
                                    }) => column_name,
                                    ColumnEntry::DerivedColumn(DerivedColumn { alias, .. }) =>
                                        alias,
                                    ColumnEntry::InternalColumn(TableInternalColumn {
                                        internal_column,
                                        ..
                                    }) => internal_column.column_name(),
                                },
                                item.index,
                                if item.asc { "ASC" } else { "DESC" }
                            ))
                            .collect::<Vec<String>>()
                            .join(", "),
                    ),
                ))),
                FormatTreeNode::new(FormatContext::Text(format!(
                    "limit: {}",
                    op.limit.map_or("NONE".to_string(), |l| l.to_string())
                ))),
            ],
            children,
        ]
        .concat(),
    )
}

pub fn logical_join_to_format_tree(
    op: &Join,
    metadata: MetadataRef,
    children: Vec<FormatTreeNode<FormatContext>>,
) -> FormatTreeNode<FormatContext> {
    let preds: Vec<ScalarExpr> = op
        .left_conditions
        .iter()
        .zip(op.right_conditions.iter())
        .map(|(left, right)| {
            ComparisonExpr {
                op: ComparisonOp::Equal,
                left: Box::new(left.clone()),
                right: Box::new(right.clone()),
            }
            .into()
        })
        .collect();
    let non_equi_conditions = op
        .non_equi_conditions
        .iter()
        .map(|scalar| format_scalar(&metadata, scalar))
        .collect::<Vec<String>>();

    let equi_conditions = if !preds.is_empty() {
        let pred = preds.iter().skip(1).fold(preds[0].clone(), |prev, next| {
            ScalarExpr::AndExpr(AndExpr {
                left: Box::new(prev),
                right: Box::new(next.clone()),
            })
        });
        format_scalar(&metadata, &pred)
    } else {
        "".to_string()
    };

    FormatTreeNode::with_children(
        FormatContext::RelOp {
            metadata,
            rel_operator: Box::new(op.clone().into()),
        },
        vec![
            vec![
                FormatTreeNode::new(FormatContext::Text(format!(
                    "equi conditions: [{}]",
                    equi_conditions
                ))),
                FormatTreeNode::new(FormatContext::Text(format!(
                    "non-equi conditions: [{}]",
                    non_equi_conditions.join(", ")
                ))),
            ],
            children,
        ]
        .concat(),
    )
}

#[allow(unused)]
fn join_to_format_tree(
    op: &Join,
    metadata: MetadataRef,
    children: Vec<FormatTreeNode<FormatContext>>,
) -> FormatTreeNode<FormatContext> {
    let build_keys = op
        .right_conditions
        .iter()
        .map(|scalar| format_scalar(&metadata, scalar))
        .collect::<Vec<String>>()
        .join(", ");
    let probe_keys = op
        .left_conditions
        .iter()
        .map(|scalar| format_scalar(&metadata, scalar))
        .collect::<Vec<String>>()
        .join(", ");
    let join_filters = op
        .non_equi_conditions
        .iter()
        .map(|scalar| format_scalar(&metadata, scalar))
        .collect::<Vec<String>>()
        .join(", ");

    FormatTreeNode::with_children(
        FormatContext::RelOp {
            metadata,
            rel_operator: Box::new(op.clone().into()),
        },
        vec![
            vec![
                FormatTreeNode::new(FormatContext::Text(format!("build keys: [{}]", build_keys))),
                FormatTreeNode::new(FormatContext::Text(format!("probe keys: [{}]", probe_keys))),
                FormatTreeNode::new(FormatContext::Text(format!(
                    "other filters: [{}]",
                    join_filters
                ))),
            ],
            children,
        ]
        .concat(),
    )
}

fn aggregate_to_format_tree(
    op: &Aggregate,
    metadata: MetadataRef,
    children: Vec<FormatTreeNode<FormatContext>>,
) -> FormatTreeNode<FormatContext> {
    let group_items = op
        .group_items
        .iter()
        .map(|item| format_scalar(&metadata, &item.scalar))
        .collect::<Vec<String>>()
        .join(", ");
    let agg_funcs = op
        .aggregate_functions
        .iter()
        .map(|item| format_scalar(&metadata, &item.scalar))
        .collect::<Vec<String>>()
        .join(", ");
    FormatTreeNode::with_children(
        FormatContext::RelOp {
            metadata,
            rel_operator: Box::new(op.clone().into()),
        },
        vec![
            vec![
                FormatTreeNode::new(FormatContext::Text(format!(
                    "group items: [{}]",
                    group_items
                ))),
                FormatTreeNode::new(FormatContext::Text(format!(
                    "aggregate functions: [{}]",
                    agg_funcs
                ))),
            ],
            children,
        ]
        .concat(),
    )
}

fn filter_to_format_tree(
    op: &Filter,
    metadata: MetadataRef,
    children: Vec<FormatTreeNode<FormatContext>>,
) -> FormatTreeNode<FormatContext> {
    let scalars = op
        .predicates
        .iter()
        .map(|scalar| format_scalar(&metadata, scalar))
        .collect::<Vec<String>>()
        .join(", ");
    FormatTreeNode::with_children(
        FormatContext::RelOp {
            metadata,
            rel_operator: Box::new(op.clone().into()),
        },
        vec![
            vec![FormatTreeNode::new(FormatContext::Text(format!(
                "filters: [{}]",
                scalars
            )))],
            children,
        ]
        .concat(),
    )
}

fn eval_scalar_to_format_tree(
    op: &EvalScalar,
    metadata: MetadataRef,
    children: Vec<FormatTreeNode<FormatContext>>,
) -> FormatTreeNode<FormatContext> {
    let scalars = op
        .items
        .iter()
        .sorted_by(|a, b| a.index.cmp(&b.index))
        .map(|item| format_scalar(&metadata, &item.scalar))
        .collect::<Vec<String>>()
        .join(", ");
    FormatTreeNode::with_children(
        FormatContext::RelOp {
            metadata,
            rel_operator: Box::new(op.clone().into()),
        },
        vec![
            vec![FormatTreeNode::new(FormatContext::Text(format!(
                "scalars: [{}]",
                scalars
            )))],
            children,
        ]
        .concat(),
    )
}

fn sort_to_format_tree(
    op: &Sort,
    metadata: MetadataRef,
    children: Vec<FormatTreeNode<FormatContext>>,
) -> FormatTreeNode<FormatContext> {
    let scalars = op
        .items
        .iter()
        .map(|item| {
            let metadata = metadata.read();
            let name = match metadata.column(item.index) {
                ColumnEntry::BaseTableColumn(BaseTableColumn { column_name, .. }) => column_name,
                ColumnEntry::DerivedColumn(DerivedColumn { alias, .. }) => alias,
                ColumnEntry::InternalColumn(TableInternalColumn {
                    internal_column, ..
                }) => internal_column.column_name(),
            };
            format!(
                "{} (#{}) {}",
                name,
                item.index,
                if item.asc { "ASC" } else { "DESC" }
            )
        })
        .collect::<Vec<String>>()
        .join(", ");
    let limit = op.limit.map_or("NONE".to_string(), |l| l.to_string());

    FormatTreeNode::with_children(
        FormatContext::RelOp {
            metadata,
            rel_operator: Box::new(op.clone().into()),
        },
        vec![
            vec![
                FormatTreeNode::new(FormatContext::Text(format!("sort keys: [{}]", scalars))),
                FormatTreeNode::new(FormatContext::Text(format!("limit: [{}]", limit))),
            ],
            children,
        ]
        .concat(),
    )
}

fn limit_to_format_tree(
    op: &Limit,
    metadata: MetadataRef,
    children: Vec<FormatTreeNode<FormatContext>>,
) -> FormatTreeNode<FormatContext> {
    let limit = if let Some(val) = op.limit { val } else { 0 };
    FormatTreeNode::with_children(
        FormatContext::RelOp {
            metadata,
            rel_operator: Box::new(op.clone().into()),
        },
        vec![
            vec![
                FormatTreeNode::new(FormatContext::Text(format!("limit: [{}]", limit))),
                FormatTreeNode::new(FormatContext::Text(format!("offset: [{}]", op.offset))),
            ],
            children,
        ]
        .concat(),
    )
}

fn exchange_to_format_tree(
    op: &Exchange,
    metadata: MetadataRef,
    children: Vec<FormatTreeNode<FormatContext>>,
) -> FormatTreeNode<FormatContext> {
    match op {
        Exchange::Hash(keys) => FormatTreeNode::with_children(
            FormatContext::RelOp {
                metadata: metadata.clone(),
                rel_operator: Box::new(op.clone().into()),
            },
            vec![
                vec![FormatTreeNode::new(FormatContext::Text(format!(
                    "Exchange(Hash): keys: [{}]",
                    keys.iter()
                        .map(|scalar| format_scalar(&metadata, scalar))
                        .collect::<Vec<String>>()
                        .join(", ")
                )))],
                children,
            ]
            .concat(),
        ),
        _ => FormatTreeNode::with_children(
            FormatContext::RelOp {
                metadata,
                rel_operator: Box::new(op.clone().into()),
            },
            children,
        ),
    }
}
