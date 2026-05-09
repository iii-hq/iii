// Copyright Motia LLC and/or licensed to Motia LLC under one or more
// contributor license agreements. Licensed under the Elastic License 2.0;
// you may not use this file except in compliance with the Elastic License 2.0.
// This software is patent protected. We welcome discussions - reach out at team@iii.dev
// See LICENSE and PATENTS files for details.

use std::time::Duration;

pub const DEFAULT_REDIS_CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);

// Validation bounds mirrored from `engine/src/update_ops.rs`. If you
// change one side, change both.
//   MAX_PATH_DEPTH    = 32
//   MAX_SEGMENT_BYTES = 256
//   MAX_VALUE_DEPTH   = 16
//   MAX_VALUE_KEYS    = 1024
// Prototype-pollution sinks: __proto__, constructor, prototype.
pub const JSON_UPDATE_SCRIPT: &str = r#"
    local json_decode = cjson.decode
    local json_encode = cjson.encode

    local MAX_PATH_DEPTH = 32
    local MAX_SEGMENT_BYTES = 256
    local MAX_VALUE_DEPTH = 16
    local MAX_VALUE_KEYS = 1024
    local PROTO = { __proto__ = true, constructor = true, prototype = true }
    local DOC_URL = 'https://iii.dev/docs/workers/iii-state#error-codes'

    local key = KEYS[1]
    local field = ARGV[1]
    local ops_json = ARGV[2]

    local old_value_str = redis.call('HGET', key, field)
    local old_value = {}
    if old_value_str then
        local ok, decoded = pcall(json_decode, old_value_str)
        if ok then
            old_value = decoded
        else
            return {'false', 'failed to decode existing JSON: ' .. tostring(decoded)}
        end
    end

    local ops = json_decode(ops_json)
    local current = json_decode(json_encode(old_value))
    local using_missing_default = old_value_str == nil
    local errors = {}

    -- get_path for legacy non-merge ops: collapses anything to a single
    -- first-level key for backward compat.
    local function get_path(path)
        if path == nil then return nil end
        if type(path) == 'string' then return path end
        if type(path) == 'table' then
            if path[1] then return path[1] end
            if path['0'] then return path['0'] end
        end
        return path
    end

    -- merge_path_segments: returns a Lua array of literal segments, or
    -- empty array meaning "root merge".
    local function merge_path_segments(path)
        if path == nil then return {} end
        if type(path) == 'string' then
            if path == '' then return {} end
            return { path }
        end
        if type(path) == 'table' then
            local out = {}
            for i, seg in ipairs(path) do
                out[i] = seg
            end
            return out
        end
        return {}
    end

    local function push_error(op_index, code, message)
        errors[#errors + 1] = {
            op_index = op_index,
            code = code,
            message = message,
            doc_url = DOC_URL,
        }
    end

    local function path_error_code(op_name, reason)
        return op_name .. '.path.' .. reason
    end

    local function json_type_name(value)
        if value == cjson.null then return 'null' end
        local value_type = type(value)
        if value_type == 'boolean' then return 'boolean' end
        if value_type == 'number' then return 'number' end
        if value_type == 'string' then return 'string' end
        if value_type == 'table' then
            if value[1] ~= nil then return 'array' end
            return 'object'
        end
        return value_type
    end

    local function path_label(path)
        if path == nil or path == '' then return 'root' end
        return path
    end

    -- Bracket-notation label for nested-segment paths. Mirrors the Rust
    -- helper `path_label_segments` in `engine/src/update_ops.rs` so the
    -- error messages produced by the two adapters match byte-for-byte.
    local function path_label_segments(segments)
        if segments == nil or #segments == 0 then return 'root' end
        return '[' .. table.concat(segments, ', ') .. ']'
    end

    local function field_path_segments(path)
        if path == nil or path == '' then return {} end
        return { path }
    end

    local function json_depth(value)
        if type(value) ~= 'table' then return 0 end
        local max = 0
        for _, v in pairs(value) do
            local d = json_depth(v)
            if d > max then max = d end
        end
        return 1 + max
    end

    local function validate_op_path(op_name, op_index, segments)
        if #segments > MAX_PATH_DEPTH then
            push_error(op_index, path_error_code(op_name, 'too_deep'),
                'Path depth ' .. #segments .. ' exceeds maximum of ' .. MAX_PATH_DEPTH)
            return false
        end
        for _, seg in ipairs(segments) do
            if type(seg) ~= 'string' or seg == '' then
                push_error(op_index, path_error_code(op_name, 'empty_segment'),
                    'Path contains an empty or non-string segment')
                return false
            end
            if #seg > MAX_SEGMENT_BYTES then
                push_error(op_index, path_error_code(op_name, 'segment_too_long'),
                    'Path segment of ' .. #seg .. ' bytes exceeds maximum of ' .. MAX_SEGMENT_BYTES)
                return false
            end
            if PROTO[seg] then
                push_error(op_index, path_error_code(op_name, 'proto_polluted'),
                    "Path segment '" .. seg .. "' is not allowed (prototype pollution).")
                return false
            end
        end
        return true
    end

    local function validate_merge_path(op_index, segments)
        return validate_op_path('merge', op_index, segments)
    end

    local function validate_merge_value(op_index, value)
        if type(value) ~= 'table' or value == cjson.null then
            push_error(op_index, 'merge.value.not_an_object',
                'Merge value must be a JSON object')
            return false
        end
        local key_count = 0
        for k, _ in pairs(value) do
            -- JSON arrays land as Lua arrays with numeric keys; reject.
            if type(k) ~= 'string' then
                push_error(op_index, 'merge.value.not_an_object',
                    'Merge value must be a JSON object')
                return false
            end
            if PROTO[k] then
                push_error(op_index, 'merge.value.proto_polluted',
                    'Merge value top-level key "' .. k .. '" is a prototype-pollution sink')
                return false
            end
            key_count = key_count + 1
            if key_count > MAX_VALUE_KEYS then
                push_error(op_index, 'merge.value.too_many_keys',
                    'Merge value has more than ' .. MAX_VALUE_KEYS .. ' top-level keys')
                return false
            end
        end
        if json_depth(value) > MAX_VALUE_DEPTH then
            push_error(op_index, 'merge.value.too_deep',
                'Merge value JSON nesting depth exceeds maximum of ' .. MAX_VALUE_DEPTH)
            return false
        end
        return true
    end

    -- "Object shape" check used at the IMP-003 root gate and inside
    -- `walk_or_create` to decide whether to walk into a node or replace
    -- it. Mirrors `json_type_name`'s convention (`value[1] ~= nil`
    -- means array, otherwise object) so empty Lua tables count as
    -- objects here. Aligns the Lua walk with the Rust path, which uses
    -- `matches!(v, Value::Object(_))` and replaces non-object
    -- intermediates (including arrays) with `Value::Object(Map::new())`.
    --
    -- Defined before `walk_or_create` (and other consumers) so the
    -- closure resolves it as an upvalue rather than a missing global.
    local function is_object_shape(value)
        if type(value) ~= 'table' or value == cjson.null then
            return false
        end
        return value[1] == nil
    end

    -- Walk segments inside `root`, replacing any non-object intermediate
    -- (null, scalar, OR array) with a fresh empty object. Mirrors the
    -- Rust `walk_or_create` (engine/src/update_ops.rs) which calls
    -- `*entry = Value::Object(Map::new())` whenever the intermediate is
    -- not already a `Value::Object(_)`. Without the array branch, Lua
    -- would walk into an existing array intermediate and produce a
    -- corrupted mixed-key form like `{1=1, 2=2, b=[42]}` for state
    -- `{"a": [1,2,3]}` + nested append `["a","b"]`.
    local function walk_or_create(root, segments)
        if not is_object_shape(root) then
            return nil  -- caller normalises root before invoking
        end
        local node = root
        for _, seg in ipairs(segments) do
            local next_node = node[seg]
            if not is_object_shape(next_node) then
                next_node = {}
                node[seg] = next_node
            end
            node = next_node
        end
        return node
    end

    local function initial_append_value(value)
        if type(value) == 'string' then
            return value
        end
        return {value}
    end

    -- Used by `append_to_target` to decide whether an existing leaf is
    -- appendable as an array. cjson loses the empty-`[]` vs empty-`{}`
    -- distinction across the encode/decode roundtrip (both materialise
    -- as a `{}` Lua table; Redis cjson does not expose `array_mt`), so
    -- this heuristic accepts empty tables as arrays. That matches the
    -- common case where users append into a freshly-stored `[]` leaf
    -- (e.g. `{"buffer": []}` + `append("buffer", x)`); the dual case
    -- where the leaf is stored as `{}` is documented as a known
    -- limitation of the Lua path. The IMP-003 root gate and
    -- `walk_or_create` use `is_object_shape` instead, so empty-document
    -- nested append still works.
    local function is_array(value)
        if type(value) ~= 'table' then
            return false
        end
        local max = 0
        local count = 0
        for k, _ in pairs(value) do
            if type(k) ~= 'number' or k < 1 or math.floor(k) ~= k then
                return false
            end
            if k > max then
                max = k
            end
            count = count + 1
        end
        return count == max
    end

    local function append_to_target(target, value, path, op_index)
        if target == nil or target == cjson.null then
            return true, initial_append_value(value)
        end
        if type(target) == 'string' then
            if type(value) == 'string' then
                return true, target .. value
            end
            push_error(op_index, 'append.type_mismatch',
                "Expected string append value at path '" .. path_label(path) .. "', got " .. json_type_name(value) .. ".")
            return false, target
        end
        if is_array(target) then
            table.insert(target, value)
            return true, target
        end
        push_error(op_index, 'append.type_mismatch',
            "Cannot append at path '" .. path_label(path) .. "': target is " .. json_type_name(target) .. ", expected array, string, null, or missing field.")
        return false, target
    end

    for op_index, op in ipairs(ops) do
        -- ipairs yields 1-based; mirror the engine's 0-based op_index.
        local zero_index = op_index - 1
        if op.type == 'set' then
            local path = get_path(op.path)
            if validate_op_path('set', zero_index, field_path_segments(path)) then
              if (path == '' or path == nil) and op.value ~= nil then
                current = op.value
                using_missing_default = false
              elseif type(current) == 'table' and current ~= cjson.null then
                if op.value == nil then
                    current[path] = cjson.null
                else
                    current[path] = op.value
                end
                using_missing_default = false
              else
                push_error(zero_index, 'set.target_not_object',
                    "Cannot set at path '" .. path_label(path) .. "': target is " .. json_type_name(current) .. ", expected object.")
              end
            end
        elseif op.type == 'merge' then
            local segments = merge_path_segments(op.path)
            if validate_merge_path(zero_index, segments) and
               validate_merge_value(zero_index, op.value) then
                if #segments == 0 then
                    -- Root merge — preserve existing semantics.
                    if type(current) == 'table' and current ~= cjson.null then
                        for k, v in pairs(op.value) do
                            current[k] = v
                        end
                        using_missing_default = false
                    end
                else
                    if type(current) ~= 'table' or current == cjson.null then
                        current = {}
                    end
                    local target = walk_or_create(current, segments)
                    if target ~= nil then
                        for k, v in pairs(op.value) do
                            target[k] = v
                        end
                        using_missing_default = false
                    end
                end
            end
        elseif op.type == 'increment' then
            local path = get_path(op.path)
            if validate_op_path('increment', zero_index, field_path_segments(path)) then
              if path == '' or path == nil then
                if using_missing_default then
                    current = op.by
                    using_missing_default = false
                elseif type(current) == 'number' then
                    current = current + op.by
                    using_missing_default = false
                else
                    push_error(zero_index, 'increment.not_number',
                        "Expected number at path '" .. path_label(path) .. "', got " .. json_type_name(current) .. ".")
                end
              elseif type(current) == 'table' and current ~= cjson.null then
                local val = current[path]
                if val == nil then
                    current[path] = op.by
                    using_missing_default = false
                elseif type(val) == 'number' then
                    current[path] = val + op.by
                    using_missing_default = false
                else
                    push_error(zero_index, 'increment.not_number',
                        "Expected number at path '" .. path_label(path) .. "', got " .. json_type_name(val) .. ".")
                end
              else
                push_error(zero_index, 'increment.target_not_object',
                    "Cannot increment at path '" .. path_label(path) .. "': target is " .. json_type_name(current) .. ", expected object.")
              end
            end
        elseif op.type == 'decrement' then
            local path = get_path(op.path)
            if validate_op_path('decrement', zero_index, field_path_segments(path)) then
              if path == '' or path == nil then
                if using_missing_default then
                    current = -op.by
                    using_missing_default = false
                elseif type(current) == 'number' then
                    current = current - op.by
                    using_missing_default = false
                else
                    push_error(zero_index, 'decrement.not_number',
                        "Expected number at path '" .. path_label(path) .. "', got " .. json_type_name(current) .. ".")
                end
              elseif type(current) == 'table' and current ~= cjson.null then
                local val = current[path]
                if val == nil then
                    current[path] = -op.by
                    using_missing_default = false
                elseif type(val) == 'number' then
                    current[path] = val - op.by
                    using_missing_default = false
                else
                    push_error(zero_index, 'decrement.not_number',
                        "Expected number at path '" .. path_label(path) .. "', got " .. json_type_name(val) .. ".")
                end
              else
                push_error(zero_index, 'decrement.target_not_object',
                    "Cannot decrement at path '" .. path_label(path) .. "': target is " .. json_type_name(current) .. ", expected object.")
              end
            end
        elseif op.type == 'append' then
            -- Validation order is load-bearing (mirror of update_ops.rs):
            --   1. validate_op_path  (bounds + proto-pollution)
            --   2. root-is-object    (before walk_or_create can mutate)
            --   3. walk_or_create    (nested only)
            --   4. leaf-type matrix  (FR-11)
            local segments = merge_path_segments(op.path)
            if validate_op_path('append', zero_index, segments) then
              if #segments == 0 then
                -- Root append: legacy semantics preserved.
                local target_root = using_missing_default and cjson.null or current
                local changed, next_value = append_to_target(target_root, op.value, 'root', zero_index)
                if changed then
                    current = next_value
                    using_missing_default = false
                end
              elseif not is_object_shape(current) then
                -- Non-empty path requires object root (IMP-003). Empty
                -- Lua tables count as objects per `is_object_shape`, so
                -- empty-document nested append succeeds (matching the
                -- Rust path's `Value::Object({})` initialization).
                push_error(zero_index, 'append.target_not_object',
                    "Cannot append at path '" .. path_label_segments(segments) .. "': target is " .. json_type_name(current) .. ", expected object.")
              elseif #segments == 1 then
                -- Single-segment path: back-compat with the legacy
                -- single-string `FieldPath` semantics — `initial_append_value`
                -- keeps the string-concat tier for missing leaves.
                local leaf_key = segments[1]
                local existing_val = current[leaf_key]
                if existing_val ~= nil and existing_val ~= cjson.null then
                    local changed, next_value = append_to_target(existing_val, op.value, leaf_key, zero_index)
                    if changed then
                        current[leaf_key] = next_value
                    end
                else
                    current[leaf_key] = initial_append_value(op.value)
                end
                using_missing_default = false
              else
                -- Nested path: walk parent (creating intermediates), then
                -- operate on the leaf key. FR-11 nested-path rule: missing
                -- leaf is ALWAYS an array (no string-concat tier).
                local parent_segments = {}
                for i = 1, #segments - 1 do
                    parent_segments[i] = segments[i]
                end
                local leaf_key = segments[#segments]
                local parent_map = walk_or_create(current, parent_segments)
                if parent_map ~= nil then
                    local existing_val = parent_map[leaf_key]
                    if existing_val ~= nil and existing_val ~= cjson.null then
                        local changed, next_value = append_to_target(existing_val, op.value, leaf_key, zero_index)
                        if changed then
                            parent_map[leaf_key] = next_value
                        end
                    else
                        -- FR-11: nested-path missing leaf is always an array.
                        parent_map[leaf_key] = { op.value }
                    end
                    using_missing_default = false
                end
              end
            end
        elseif op.type == 'remove' then
            local path = get_path(op.path)
            if validate_op_path('remove', zero_index, field_path_segments(path)) then
              if path == '' or path == nil then
                current = cjson.null
                using_missing_default = false
              elseif type(current) == 'table' and current ~= cjson.null then
                current[path] = nil
                using_missing_default = false
              else
                push_error(zero_index, 'remove.target_not_object',
                    "Cannot remove at path '" .. path_label(path) .. "': target is " .. json_type_name(current) .. ", expected object.")
              end
            end
        end
    end

    local new_value_str = json_encode(current)
    redis.call('HSET', key, field, new_value_str)

    -- Return tuple shape:
    --   {'true', old_value_str, new_value_str, errors_json}
    -- errors_json is omitted (4 elements only when present) when no
    -- errors occurred, preserving backward compatibility with adapters
    -- that expect 3 elements.
    if #errors == 0 then
        return {'true', old_value_str or '', new_value_str}
    else
        return {'true', old_value_str or '', new_value_str, json_encode(errors)}
    end
"#;
