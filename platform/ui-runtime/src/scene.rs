/*
 * Licensed to the Apache Software Foundation (ASF) under one
 * or more contributor license agreements.  See the NOTICE file
 * distributed with this work for additional information
 * regarding copyright ownership.  The ASF licenses this file
 * to you under the Apache License, Version 2.0 (the
 * "License"); you may not use this file except in compliance
 * with the License.  You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing,
 * software distributed under the License is distributed on an
 * "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
 * KIND, either express or implied.  See the License for the
 * specific language governing permissions and limitations
 * under the License.
 */

//! Pure presentation protocols shared by schema generation and native renderers.
pub mod terminal;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn responsive_stack_validates_fraction_bounds_and_direct_child_overrides() {
        let base = json!({"kind":"stack","key":"root","direction":"column","gap":0,"padding":0,"background":0,
            "wide":{"min_width":100,"direction":"row","children":{"list":{"basis_fraction":{"numerator":34,"denominator":100}}}},
            "children":[{"kind":"text","key":"list","text":"x","color":0}]});
        let validate = |value| {
            serde_json::from_value::<terminal::Node>(value)
                .unwrap()
                .validate_root()
        };
        validate(base.clone()).unwrap();
        for invalid in [
            json!({"numerator":1,"denominator":0}),
            json!({"numerator":2,"denominator":1}),
            json!({"numerator":1,"denominator":3,"min":6,"max":5}),
        ] {
            let mut value = base.clone();
            value["wide"]["children"]["list"]["basis_fraction"] = invalid.clone();
            assert!(validate(value).is_err());
            let mut value = base.clone();
            value["layout"] = json!({"basis_fraction":invalid});
            assert!(validate(value).is_err());
        }
        for (pointer, invalid) in [
            ("/wide/min_width", json!(0)),
            ("/wide/children", json!({"missing":{"basis":10}})),
            (
                "/wide/children/list",
                json!({"basis":1,"basis_fraction":{"numerator":1,"denominator":2}}),
            ),
            ("/wide/children/list", json!({"wrap":true})),
        ] {
            let mut value = base.clone();
            *value.pointer_mut(pointer).unwrap() = invalid;
            assert!(validate(value).is_err());
        }
        let mut value = base;
        value["scroll"] = json!("manual");
        assert!(
            validate(value).is_err(),
            "a scrolling column cannot switch to a row"
        );
    }

    #[test]
    fn wheel_commands_require_a_viewport_and_share_the_original_input_contract() {
        let value = json!({"kind":"stack","key":"list","direction":"column","gap":0,"padding":0,"background":0,"children":[],"scroll":"manual","wheel":{"up":"previous","down":"next"}});
        let node: terminal::Node = serde_json::from_value(value.clone()).unwrap();
        node.validate_root().unwrap();
        let mut inputs = std::collections::BTreeMap::new();
        inputs = node.metadata(inputs).unwrap().inputs;
        assert_eq!(inputs.len(), 2);
        assert_eq!(inputs["next"], json!(true));
        inputs.insert("next".into(), json!({"type":"string"}));
        assert!(node.metadata(inputs).is_err());
        for (field, replacement) in [
            ("scroll", json!("none")),
            ("wheel", json!({"up":"","down":"next"})),
        ] {
            let mut invalid = value.clone();
            invalid[field] = replacement;
            let node: terminal::Node = serde_json::from_value(invalid).unwrap();
            assert!(node.validate_root().is_err());
        }
        let mut complete = value.clone();
        complete["wheel"]["down"] = json!("x".repeat(129));
        complete["padding"] = json!(u16::MAX);
        complete["gap"] = json!(u16::MAX);
        let node: terminal::Node = serde_json::from_value(complete.clone()).unwrap();
        node.validate_root().unwrap();
        assert_eq!(
            node.metadata(Default::default()).unwrap().inputs[&"x".repeat(129)],
            json!(true)
        );
        complete["padding"] = json!(u32::from(u16::MAX) + 1);
        assert!(
            serde_json::from_value::<terminal::Node>(complete).is_err(),
            "native geometry remains representable as u16"
        );
    }

    #[test]
    fn parent_commands_check_native_inputs_in_later_descendants() {
        let text = |key| json!({"kind":"text","key":key,"text":"Body","color":0});
        let mut source = json!({"kind":"stack","key":"root","direction":"column","gap":0,"padding":0,"background":0,
            "shortcuts":[{"key":"k","modifiers":["primary"],"input":"target","enabled":true}],
            "children":[{"kind":"item","key":"target","content":text("body")}]});
        let terminal = |source| {
            let node: terminal::Node = serde_json::from_value(source).unwrap();
            node.validate_root().unwrap();
            node.metadata(Default::default())
        };
        assert_eq!(
            terminal(source.clone()).err().unwrap(),
            "shortcut collides with typed native input"
        );
        source["shortcuts"] = json!([]);
        source["scroll"] = json!("manual");
        source["wheel"] = json!({"up":"target","down":"next"});
        assert_eq!(
            terminal(source.clone()).err().unwrap(),
            "wheel command collides with typed native input"
        );
        source["shortcuts"] =
            json!([{"key":"k","modifiers":["primary"],"input":"target","enabled":true}]);
        source.as_object_mut().unwrap().remove("wheel");
        source["children"] = json!([{"kind":"button","key":"target","label":"Action"}]);
        assert_eq!(
            terminal(source.clone()).unwrap().inputs["target"],
            json!(true)
        );
    }

    #[test]
    fn copy_button_keeps_complete_text_and_no_conflicting_native_default() {
        {
            let text = String::new();
            let value = json!({"kind":"button","key":"copy","label":"Copy","copy_text":text});
            assert!(
                serde_json::from_value::<terminal::Node>(value.clone())
                    .unwrap()
                    .validate_root()
                    .is_err()
            );
        }
        let value =
            json!({"kind":"button","key":"copy","label":"Copy","copy_text":"界".repeat(24 * 1024)});
        serde_json::from_value::<terminal::Node>(value.clone())
            .unwrap()
            .validate_root()
            .unwrap();
        let mut conflict = value;
        conflict["focus_child"] = json!("editor");
        assert!(
            serde_json::from_value::<terminal::Node>(conflict)
                .unwrap()
                .validate_root()
                .is_err()
        );
    }
}
