package live

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
)

func writeFixture(home, cwd string, options Options) error {
	model := map[string]any{
		"slug": options.Model, "display_name": options.Model, "supported_reasoning_levels": []any{},
		"shell_type": "disabled", "visibility": "list", "supported_in_api": true, "priority": 0,
		"model_messages":    map[string]any{"instructions_template": "Answer the user directly."},
		"support_verbosity": false, "apply_patch_tool_type": nil,
		"truncation_policy":            map[string]any{"mode": "bytes", "limit": 10000},
		"experimental_supported_tools": []any{}, "context_window": 131072,
		"supports_reasoning_summary_parameter": false, "include_apps_usage_instructions": false,
		"node_repl_disabled": true, "tool_mode": "direct",
	}
	catalog, _ := json.Marshal(map[string]any{"models": []any{model}})
	catalogPath := filepath.Join(home, "models.json")
	profile := fmt.Sprintf(`model = %q
model_provider = "grok"
model_catalog_json = %q
approval_policy = "never"
sandbox_mode = "read-only"
web_search = "disabled"
[model_providers.grok]
name = "Grok"
base_url = %q
env_key = "GROK_API_KEY"
wire_api = "grok_responses"
[tools.update_plan]
enabled = false
[tools.experimental_request_user_input]
enabled = false
[features]
goals = false
shell_tool = false
view_image = false
sleep_tool = false
multi_agent = false
multi_agent_v2 = false
code_mode = false
apps = false
image_generation = false
tool_suggest = false
current_time_reminder = false
send_message_to_user_async = false
token_budget = false
request_permissions_tool = false
deferred_executor = false
`, options.Model, catalogPath, options.BaseURL)
	if os.Mkdir(cwd, 0700) != nil || os.WriteFile(catalogPath, catalog, 0600) != nil || os.WriteFile(filepath.Join(home, "config.toml"), []byte(profile), 0600) != nil {
		return errors.New("live: isolated fixture creation failed")
	}
	return nil
}
