use crate::schema::Schema;
use dashmap::DashMap;
use serde_json::Value;

pub struct TriggerType {
    pub id: String,
    pub description: String,
    // pub config_schema: Schema,
    pub on_register: Box<dyn Fn(&Trigger) -> Result<(), anyhow::Error> + Send + Sync>,
    pub on_unregister: Box<dyn Fn(&Trigger) -> Result<(), anyhow::Error> + Send + Sync>,
}

pub struct Trigger {
    pub id: String,
    pub trigger_type: String,
    pub function_path: String,
    pub config: Value,
}

#[derive(Default)]
pub struct TriggerRegistry {
    trigger_types: DashMap<String, TriggerType>,
    triggers: DashMap<String, Trigger>,
}

impl TriggerRegistry {
    pub fn new() -> Self {
        Self {
            trigger_types: DashMap::new(),
            triggers: DashMap::new(),
        }
    }

    pub fn register_trigger_type(&self, trigger_type: TriggerType) {
        let trigger_type_id = &trigger_type.id;
        let on_register = &trigger_type.on_register;

        for pair in self.triggers.iter() {
            let trigger = pair.value();

            if &trigger.trigger_type == trigger_type_id {
                let result = on_register(trigger);
                if let Err(err) = result {
                    eprintln!("Error registering trigger: {}", err);
                }
            }
        }

        self.trigger_types
            .insert(trigger_type.id.clone(), trigger_type);
    }

    pub fn unregister_trigger_type(&self, id: String) {
        self.trigger_types.remove(&id);
    }

    pub fn register_trigger(&self, trigger: Trigger) -> Result<(), anyhow::Error> {
        let Some(trigger_type) = self.trigger_types.get(&trigger.trigger_type.clone()) else {
            return Err(anyhow::anyhow!("Trigger type not found"));
        };
        let result = (trigger_type.on_register)(&trigger);

        if result.is_err() {
            return Err(result.err().unwrap());
        }

        self.triggers.insert(trigger.id.clone(), trigger);

        Ok(())
    }

    pub fn unregister_trigger(&self, trigger: Trigger) -> Result<(), anyhow::Error> {
        let Some(trigger) = self.triggers.get(&trigger.id) else {
            return Err(anyhow::anyhow!("Trigger not found"));
        };
        let trigger_type = self.trigger_types.get(&trigger.trigger_type.clone());

        if trigger_type.is_some() {
            let result = (trigger_type.unwrap().on_unregister)(&trigger);

            if result.is_err() {
                return Err(result.err().unwrap());
            }
        }

        self.triggers.remove(&trigger.id);
        Ok(())
    }
}
