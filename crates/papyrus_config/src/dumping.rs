//! Utils for serializing config objects into flatten map and json file.
//! The elements structure is:
//!
//! ```json
//! "conf1.conf2.conf3.param_name": {
//!     "description": "Param description.",
//!     "value": json_value
//! }
//! ```
//! In addition, supports pointers in the map, with the structure:
//!
//! ```json
//! "conf1.conf2.conf3.param_name": {
//!     "description": "Param description.",
//!     "pointer_target": "target_param_path"
//! }
//! ```
//!
//! Supports flags for optional params and sub-configs. An optional param / sub-config has an
//! "#is_none" indicator that determines whether to take its value or to deserialize it to None:
//! ```json
//! "conf1.conf2.#is_none": {
//!     "description": ""Flag for an optional field.",
//!     "value": true
//! }
//! ```

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufWriter, Write};

use itertools::chain;
use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::{ConfigError, ParamPath, PointerParam, SerializedParam, IS_NONE_MARK};

/// Serialization for configs.
pub trait SerializeConfig {
    /// Conversion of a configuration to a mapping of flattened parameters to their descriptions and
    /// values.
    /// Note, in the case of a None sub configs, its elements will not included in the flatten map.
    fn dump(&self) -> BTreeMap<ParamPath, SerializedParam>;

    /// Serialization of a configuration into a JSON file.
    /// Takes a vector of {target pointer params, SerializedParam, and vector of pointing params},
    /// adds the target pointer params with the description and a value, and replaces the value of
    /// the pointing params to contain only the name of the target they point to.
    ///
    /// Note, in the case of a None sub configs, its elements will not included in the file.
    fn dump_to_file(
        &self,
        config_pointers: &Vec<((ParamPath, SerializedParam), Vec<ParamPath>)>,
        file_path: &str,
    ) -> Result<(), ConfigError> {
        let combined_map = combine_config_map_and_pointers(self.dump(), config_pointers)?;
        let file = File::create(file_path)?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer_pretty(&mut writer, &combined_map)?;
        writer.flush()?;
        Ok(())
    }
}

/// Appends `sub_config_name` to the ParamPath for each entry in `sub_config_dump`.
/// In order to load from a dump properly, `sub_config_name` must match the field's name for the
/// struct this function is called from.
pub fn append_sub_config_name(
    sub_config_dump: BTreeMap<ParamPath, SerializedParam>,
    sub_config_name: &str,
) -> BTreeMap<ParamPath, SerializedParam> {
    BTreeMap::from_iter(
        sub_config_dump
            .into_iter()
            .map(|(field_name, val)| (format!("{}.{}", sub_config_name, field_name), val)),
    )
}

/// Serializes a single param of a config.
/// The returned pair is designed to be an input to a dumped config map.
pub fn ser_param<T: Serialize>(
    name: &str,
    value: &T,
    description: &str,
) -> (String, SerializedParam) {
    (name.to_owned(), SerializedParam { description: description.to_owned(), value: json!(value) })
}

/// Serializes optional sub-config fields (or default fields for None sub-config) and adds an
/// "#is_none" flag.
pub fn ser_optional_sub_config<T: SerializeConfig + Default>(
    optional_config: &Option<T>,
    name: &str,
) -> BTreeMap<ParamPath, SerializedParam> {
    chain!(
        BTreeMap::from_iter([ser_is_param_none(name, optional_config.is_none())]),
        append_sub_config_name(
            match optional_config {
                None => T::default().dump(),
                Some(config) => config.dump(),
            },
            name,
        ),
    )
    .collect()
}

/// Serializes optional param value (or default value for None param) and adds an "#is_none" flag.
pub fn ser_optional_param<T: Serialize>(
    optional_param: &Option<T>,
    default_value: T,
    name: &str,
    description: &str,
) -> BTreeMap<ParamPath, SerializedParam> {
    BTreeMap::from([
        ser_is_param_none(name, optional_param.is_none()),
        ser_param(
            name,
            match optional_param {
                Some(param) => param,
                None => &default_value,
            },
            description,
        ),
    ])
}

/// Serializes is_none flag for a param.
pub fn ser_is_param_none(name: &str, is_none: bool) -> (String, SerializedParam) {
    (
        format!("{name}.{IS_NONE_MARK}"),
        SerializedParam {
            description: "Flag for an optional field".to_owned(),
            value: json!(is_none),
        },
    )
}

// Takes a config map and a vector of {target param, serialized pointer, and vector of params that
// will point to it}.
// Adds to the map the target params.
// Replaces the value of the pointers to contain only the name of the target they point to.
pub(crate) fn combine_config_map_and_pointers(
    config_map: BTreeMap<ParamPath, SerializedParam>,
    pointers: &Vec<((ParamPath, SerializedParam), Vec<ParamPath>)>,
) -> Result<Value, ConfigError> {
    let mut json_val = serde_json::to_value(config_map).unwrap();
    let json_map: &mut serde_json::Map<std::string::String, serde_json::Value> =
        json_val.as_object_mut().unwrap();

    for ((target_param, serialized_pointer), pointing_params_vec) in pointers {
        json_map.insert(target_param.clone(), json!(serialized_pointer));

        for pointing_param in pointing_params_vec {
            let pointing_serialized_param = get_serialized_param(pointing_param, json_map)?;
            json_map.remove(pointing_param);
            json_map.insert(
                pointing_param.to_owned(),
                json!(PointerParam {
                    description: pointing_serialized_param.description,
                    pointer_target: target_param.to_owned()
                }),
            );
        }
    }
    Ok(json_val)
}

fn get_serialized_param(
    param_path: &ParamPath,
    json_map: &Map<String, Value>,
) -> Result<SerializedParam, ConfigError> {
    if let Some(json_value) = json_map.get(param_path) {
        Ok(serde_json::from_value::<SerializedParam>(json_value.clone())?)
    } else {
        Err(ConfigError::PointerSourceNotFound { pointing_param: param_path.to_owned() })
    }
}