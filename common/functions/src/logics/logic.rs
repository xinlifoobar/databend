// Copyright 2020-2021 The Datafuse Authors.
//
// SPDX-License-Identifier: Apache-2.0.

use std::fmt;

use common_datavalues::columns::DataColumn;
use common_datavalues::DataSchema;
use common_datavalues::DataType;
use common_datavalues::DataValueLogicOperator;
use common_exception::Result;

use crate::logics::LogicAndFunction;
use crate::logics::LogicNotFunction;
use crate::logics::LogicOrFunction;
use crate::FactoryFuncRef;
use crate::Function;

#[derive(Clone)]
pub struct LogicFunction {
    op: DataValueLogicOperator,
}

impl LogicFunction {
    pub fn register(map: FactoryFuncRef) -> Result<()> {
        let mut map = map.write();
        map.insert("and", LogicAndFunction::try_create_func);
        map.insert("or", LogicOrFunction::try_create_func);
        map.insert("not", LogicNotFunction::try_create_func);
        Ok(())
    }

    pub fn try_create_func(op: DataValueLogicOperator) -> Result<Box<dyn Function>> {
        Ok(Box::new(LogicFunction { op }))
    }
}

impl Function for LogicFunction {
    fn name(&self) -> &str {
        "LogicFunction"
    }

    fn return_type(&self, _args: &[DataType]) -> Result<DataType> {
        Ok(DataType::Boolean)
    }

    fn nullable(&self, _input_schema: &DataSchema) -> Result<bool> {
        Ok(false)
    }

    fn eval(&self, columns: &[DataColumn], _input_rows: usize) -> Result<DataColumn> {
        columns[0].logic(self.op.clone(), columns[1..])
    }
}

impl fmt::Display for LogicFunction {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.op)
    }
}
