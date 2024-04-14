use log::info;
use miette::Result;
use nu_engine::{convert_env_values, eval_block};
use nu_parser::{escape_for_script_arg, parse};
use nu_protocol::{
    debugger::WithoutDebug,
    engine::{EngineState, Stack, StateWorkingSet},
    report_error, PipelineData, Spanned, Value,
};

/// Run a command (or commands) given to us by the user
pub fn evaluate_commands(
    commands: &Spanned<String>,
    args_to_commands: Vec<String>,
    engine_state: &mut EngineState,
    stack: &mut Stack,
    input: PipelineData,
    table_mode: Option<Value>,
    no_newline: bool,
) -> Result<Option<i64>> {
    // Translate environment variables from Strings to Values
    if let Some(e) = convert_env_values(engine_state, stack) {
        let working_set = StateWorkingSet::new(engine_state);
        report_error(&working_set, &e);
        std::process::exit(1);
    }

    // Parse the source code
    let (block, delta) = {
        if let Some(ref t_mode) = table_mode {
            let mut config = engine_state.get_config().clone();
            config.table_mode = t_mode.coerce_str()?.parse().unwrap_or_default();
            engine_state.set_config(config);
        }

        let mut working_set = StateWorkingSet::new(engine_state);

        let mut commands = commands.item.clone();
        if !args_to_commands.is_empty() {
            let args_to_commands: Vec<String> = args_to_commands
                .into_iter()
                .map(|a| escape_for_script_arg(&a))
                .collect();
            commands = format!(
                "def --wrapped main [...args] {{ {} }}; main {}",
                commands,
                args_to_commands.join(" "),
            )
        }
        let output = parse(&mut working_set, None, commands.as_bytes(), false);
        if let Some(warning) = working_set.parse_warnings.first() {
            report_error(&working_set, warning);
        }

        if let Some(err) = working_set.parse_errors.first() {
            report_error(&working_set, err);

            std::process::exit(1);
        }

        (output, working_set.render())
    };

    // Update permanent state
    if let Err(err) = engine_state.merge_delta(delta) {
        let working_set = StateWorkingSet::new(engine_state);
        report_error(&working_set, &err);
    }

    // Run the block
    let exit_code = match eval_block::<WithoutDebug>(engine_state, stack, &block, input) {
        Ok(pipeline_data) => {
            let mut config = engine_state.get_config().clone();
            if let Some(t_mode) = table_mode {
                config.table_mode = t_mode.coerce_str()?.parse().unwrap_or_default();
            }
            crate::eval_file::print_table_or_error(
                engine_state,
                stack,
                pipeline_data,
                &mut config,
                no_newline,
            )
        }
        Err(err) => {
            let working_set = StateWorkingSet::new(engine_state);

            report_error(&working_set, &err);
            std::process::exit(1);
        }
    };

    info!("evaluate {}:{}:{}", file!(), line!(), column!());

    Ok(exit_code)
}
