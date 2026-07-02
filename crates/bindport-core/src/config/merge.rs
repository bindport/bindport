use super::*;

impl OutputConfig {
    pub(crate) fn merge(&mut self, local: Self) {
        override_option(&mut self.enabled, local.enabled);
        override_option(&mut self.template, local.template);
        override_option(&mut self.root, local.root);
        override_option(&mut self.target, local.target);
        override_option(&mut self.target_host, local.target_host);
        override_option(&mut self.target_scheme, local.target_scheme);
        override_option(&mut self.auto_render, local.auto_render);
        override_option(&mut self.delete_on, local.delete_on);
        override_option(&mut self.on_failure, local.on_failure);
        override_option(&mut self.debounce_ms, local.debounce_ms);
        merge_map_option(&mut self.vars, local.vars);
    }
}

pub(crate) fn override_option<T>(base: &mut Option<T>, local: Option<T>) {
    if local.is_some() {
        *base = local;
    }
}

pub(crate) fn merge_option_with<T>(
    base: &mut Option<T>,
    local: Option<T>,
    merge: impl FnOnce(&mut T, T),
) {
    match (base.as_mut(), local) {
        (Some(base), Some(local)) => merge(base, local),
        (None, Some(local)) => *base = Some(local),
        (_, None) => {}
    }
}

pub(crate) fn merge_map_option<T>(
    base: &mut Option<BTreeMap<String, T>>,
    local: Option<BTreeMap<String, T>>,
) {
    let Some(local) = local else {
        return;
    };

    if let Some(base) = base {
        base.extend(local);
    } else {
        *base = Some(local);
    }
}

pub(crate) fn merge_outputs(
    base: &mut Option<Vec<OutputConfig>>,
    local: Option<Vec<OutputConfig>>,
) {
    let Some(local_outputs) = local else {
        return;
    };

    let Some(base_outputs) = base else {
        *base = Some(local_outputs);
        return;
    };

    for local_output in local_outputs {
        let Some(local_name) = local_output.name.as_deref() else {
            base_outputs.push(local_output);
            continue;
        };

        if let Some(base_output) = base_outputs
            .iter_mut()
            .find(|output| output.name.as_deref() == Some(local_name))
        {
            base_output.merge(local_output);
        } else {
            base_outputs.push(local_output);
        }
    }
}
