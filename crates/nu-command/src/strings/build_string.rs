use nu_engine::eval_expression;
use nu_protocol::ast::Call;
use nu_protocol::engine::{Command, EngineState, EvaluationContext, Stack};
use nu_protocol::{
    Example, IntoPipelineData, PipelineData, ShellError, Signature, Span, SyntaxShape, Value,
};

#[derive(Clone)]
pub struct BuildString;

impl Command for BuildString {
    fn name(&self) -> &str {
        "build-string"
    }

    fn usage(&self) -> &str {
        "Create a string from the arguments."
    }

    fn signature(&self) -> nu_protocol::Signature {
        Signature::build("build-string").rest("rest", SyntaxShape::String, "list of string")
    }

    fn examples(&self) -> Vec<Example> {
        vec![
            Example {
                example: "build-string a b c",
                description: "Builds a string from letters a b c",
                result: Some(Value::String {
                    val: "abc".to_string(),
                    span: Span::unknown(),
                }),
            },
            Example {
                example: "build-string (1 + 2) = one ' ' plus ' ' two",
                description: "Builds a string from letters a b c",
                result: Some(Value::String {
                    val: "3=one plus two".to_string(),
                    span: Span::unknown(),
                }),
            },
        ]
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        _input: PipelineData,
    ) -> Result<nu_protocol::PipelineData, nu_protocol::ShellError> {
        let output = call
            .positional
            .iter()
            .map(|expr| eval_expression(engine_state, stack, expr).map(|val| val.into_string()))
            .collect::<Result<Vec<String>, ShellError>>()?;

        Ok(Value::String {
            val: output.join(""),
            span: call.head,
        }
        .into_pipeline_data())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_examples() {
        use crate::test_examples;

        test_examples(BuildString {})
    }
}
