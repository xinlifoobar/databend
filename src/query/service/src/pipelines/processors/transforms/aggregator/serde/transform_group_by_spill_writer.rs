// Copyright 2023 Datafuse Labs.
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

use std::any::Any;
use std::sync::Arc;
use std::time::Instant;

use common_base::base::GlobalUniqName;
use common_exception::ErrorCode;
use common_exception::Result;
use common_expression::arrow::serialize_column;
use common_expression::BlockEntry;
use common_expression::BlockMetaInfoDowncast;
use common_expression::DataBlock;
use common_pipeline_core::processors::port::InputPort;
use common_pipeline_core::processors::port::OutputPort;
use common_pipeline_core::processors::processor::Event;
use common_pipeline_core::processors::Processor;
use futures_util::future::BoxFuture;
use opendal::Operator;
use tracing::info;

use crate::pipelines::processors::transforms::aggregator::aggregate_meta::AggregateMeta;
use crate::pipelines::processors::transforms::aggregator::aggregate_meta::HashTablePayload;
use crate::pipelines::processors::transforms::aggregator::serde::transform_group_by_serializer::serialize_group_by;
use crate::pipelines::processors::transforms::group_by::HashMethodBounds;

pub struct TransformGroupBySpillWriter<Method: HashMethodBounds> {
    method: Method,
    input: Arc<InputPort>,
    output: Arc<OutputPort>,

    operator: Operator,
    location_prefix: String,
    output_block: Option<DataBlock>,
    spilling_meta: Option<AggregateMeta<Method, ()>>,
    spilling_future: Option<BoxFuture<'static, Result<()>>>,
}

impl<Method: HashMethodBounds> TransformGroupBySpillWriter<Method> {
    pub fn create(
        input: Arc<InputPort>,
        output: Arc<OutputPort>,
        method: Method,
        operator: Operator,
        location_prefix: String,
    ) -> Box<dyn Processor> {
        Box::new(TransformGroupBySpillWriter::<Method> {
            method,
            input,
            output,
            operator,
            location_prefix,
            output_block: None,
            spilling_meta: None,
            spilling_future: None,
        })
    }
}

#[async_trait::async_trait]
impl<Method: HashMethodBounds> Processor for TransformGroupBySpillWriter<Method> {
    fn name(&self) -> String {
        String::from("TransformGroupBySpillWriter")
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }

    fn event(&mut self) -> Result<Event> {
        if self.output.is_finished() {
            self.input.finish();
            return Ok(Event::Finished);
        }

        if !self.output.can_push() {
            self.input.set_not_need_data();
            return Ok(Event::NeedConsume);
        }

        if self.spilling_future.is_some() {
            self.input.set_not_need_data();
            return Ok(Event::Async);
        }

        if let Some(spilled_meta) = self.output_block.take() {
            self.output.push_data(Ok(spilled_meta));
            return Ok(Event::NeedConsume);
        }

        if self.spilling_meta.is_some() {
            self.input.set_not_need_data();
            return Ok(Event::Sync);
        }

        if self.input.has_data() {
            let mut data_block = self.input.pull_data().unwrap()?;

            if let Some(block_meta) = data_block
                .get_meta()
                .and_then(AggregateMeta::<Method, ()>::downcast_ref_from)
            {
                if matches!(block_meta, AggregateMeta::Spilling(_)) {
                    self.input.set_not_need_data();
                    let block_meta = data_block.take_meta().unwrap();
                    self.spilling_meta = AggregateMeta::<Method, ()>::downcast_from(block_meta);
                    return Ok(Event::Sync);
                }
            }

            self.output.push_data(Ok(data_block));
            return Ok(Event::NeedConsume);
        }

        if self.input.is_finished() {
            self.output.finish();
            return Ok(Event::Finished);
        }

        self.input.set_need_data();
        Ok(Event::NeedData)
    }

    fn process(&mut self) -> Result<()> {
        if let Some(spilling_meta) = self.spilling_meta.take() {
            if let AggregateMeta::Spilling(payload) = spilling_meta {
                let (output_block, spilling_future) = spilling_group_by_payload(
                    self.operator.clone(),
                    &self.method,
                    &self.location_prefix,
                    payload,
                )?;

                self.output_block = Some(output_block);
                self.spilling_future = Some(spilling_future);

                return Ok(());
            }

            return Err(ErrorCode::Internal(
                "TransformGroupBySpillWriter only recv AggregateMeta",
            ));
        }

        Ok(())
    }

    async fn async_process(&mut self) -> Result<()> {
        if let Some(spilling_future) = self.spilling_future.take() {
            return spilling_future.await;
        }

        Ok(())
    }
}

fn get_columns(data_block: DataBlock) -> Vec<BlockEntry> {
    data_block.columns().to_vec()
}

fn serialize_spill_file<Method: HashMethodBounds>(
    method: &Method,
    payload: HashTablePayload<Method, ()>,
) -> Result<(isize, usize, Vec<Vec<u8>>)> {
    let bucket = payload.bucket;
    let data_block = serialize_group_by(method, payload)?;
    let columns = get_columns(data_block);

    let mut total_size = 0;
    let mut columns_data = Vec::with_capacity(columns.len());
    for column in columns.into_iter() {
        let column = column.value.as_column().unwrap();
        let column_data = serialize_column(column);
        total_size += column_data.len();
        columns_data.push(column_data);
    }

    Ok((bucket, total_size, columns_data))
}

pub fn spilling_group_by_payload<Method: HashMethodBounds>(
    operator: Operator,
    method: &Method,
    location_prefix: &str,
    payload: HashTablePayload<Method, ()>,
) -> Result<(DataBlock, BoxFuture<'static, Result<()>>)> {
    let (bucket, total_size, data) = serialize_spill_file(method, payload)?;

    let unique_name = GlobalUniqName::unique();
    let location = format!("{}/{}", location_prefix, unique_name);
    let columns_layout = data.iter().map(Vec::len).collect::<Vec<_>>();
    let output_data_block = DataBlock::empty_with_meta(
        AggregateMeta::<Method, ()>::create_spilled(bucket, location.clone(), columns_layout),
    );

    Ok((
        output_data_block,
        Box::pin(async move {
            let instant = Instant::now();

            // temp code: waiting https://github.com/datafuselabs/opendal/pull/1431
            let mut write_data = Vec::with_capacity(total_size);

            for data in data.into_iter() {
                write_data.extend(data);
            }

            operator.write(&location, write_data).await?;

            info!(
                "Write aggregate spill {} successfully, elapsed: {:?}",
                location,
                instant.elapsed()
            );

            Ok(())
        }),
    ))
}
