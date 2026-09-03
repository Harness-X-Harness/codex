// Command grokex-live runs one Live scenario against an immutable Grokex
// archive and writes secret-safe evidence.
//
// It drives a real task through the exact app-server protocol in a fresh
// CODEX_HOME, lets the app-server end normally, then proves the Story from the
// persisted session rollouts, the saved artifacts, and the delivered reply.
//
//	grokex-live --archive A.tar.gz --config secret.toml --evidence out.json \
//	  --source-sha S --run-id R --scenario image-generation-history-edit
package main

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"time"

	"github.com/Harness-X-Harness/codex/grokex/validator/internal/archive"
	"github.com/Harness-X-Harness/codex/grokex/validator/internal/contract"
	"github.com/Harness-X-Harness/codex/grokex/validator/internal/driver"
	"github.com/Harness-X-Harness/codex/grokex/validator/internal/evidence"
	"github.com/Harness-X-Harness/codex/grokex/validator/internal/oracle"
	"github.com/Harness-X-Harness/codex/grokex/validator/internal/rollout"
)

const (
	scenarioBasic         = "basic-exact-reply"
	scenarioContinuation  = "encrypted-reasoning-tool-continuation"
	scenarioCollaboration = "ultra-full-history-collaboration"
	scenarioImage         = "image-generation-history-edit"
	// rolloutSettle bounds how long to wait for the canonical task_complete
	// record after the app-server reported the terminal Turn.
	rolloutSettle = 15 * time.Second
)

type options struct {
	archive   string
	config    string
	evidence  string
	contracts string
	sourceSHA string
	runID     string
	scenario  string
	mode      string
}

func main() {
	os.Exit(run())
}

func run() int {
	var opts options
	flag.StringVar(&opts.archive, "archive", "", "Linux Grokex release archive (.tar.gz)")
	flag.StringVar(&opts.config, "config", "", "secret Grok profile config.toml")
	flag.StringVar(&opts.evidence, "evidence", "", "evidence JSON output path")
	flag.StringVar(&opts.contracts, "contracts", filepath.Join("grokex", "live_contracts.json"), "executable Live contract")
	flag.StringVar(&opts.sourceSHA, "source-sha", "", "source SHA of the release run")
	flag.StringVar(&opts.runID, "run-id", "", "validation run id")
	flag.StringVar(&opts.scenario, "scenario", "", "scenario id from the Live contract")
	flag.StringVar(&opts.mode, "mode", evidence.ModeRelease, "release or observation")
	flag.Parse()
	for name, value := range map[string]string{
		"archive": opts.archive, "config": opts.config, "evidence": opts.evidence,
		"source-sha": opts.sourceSHA, "run-id": opts.runID, "scenario": opts.scenario,
	} {
		if value == "" {
			fmt.Fprintf(os.Stderr, "--%s is required\n", name)
			return 2
		}
	}
	if opts.mode != evidence.ModeRelease && opts.mode != evidence.ModeObservation {
		fmt.Fprintf(os.Stderr, "unknown live mode: %s\n", opts.mode)
		return 2
	}

	contracts, err := contract.Load(opts.contracts)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		return 2
	}
	scenario, err := contracts.Scenario(opts.scenario)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		return 2
	}
	clock := evidence.NewClock()
	identity := evidence.Identity{
		Mode:          opts.mode,
		Scenario:      opts.scenario,
		Story:         scenario.Story,
		SourceSHA:     opts.sourceSHA,
		ValidationRun: opts.runID,
	}
	document, err := execute(opts, scenario, &identity, clock)
	if err != nil {
		// Harness and environment failures still leave a classified record.
		document = evidence.Document{
			Identity: identity,
			Stages:   clock.Stages(),
			Failure:  &evidence.Failure{Category: categoryOf(err), Reason: err.Error(), Stage: lastStage(clock)},
		}
	}
	if writeErr := evidence.Write(opts.evidence, document); writeErr != nil {
		fmt.Fprintln(os.Stderr, writeErr)
		return 1
	}
	if document.Failure == nil {
		fmt.Printf("%s: GREEN (%s)\n", opts.scenario, oracle.EvidenceSource)
		return 0
	}
	fmt.Fprintf(os.Stderr, "%s: %s at %s: %s\n", opts.scenario, document.Failure.Category, document.Failure.Stage, document.Failure.Reason)
	if opts.mode == evidence.ModeObservation {
		return 0
	}
	return 1
}

type environmentError struct{ error }

func terminalStage(prefix string, run driver.TurnRun) string {
	if run.DeadlineExpired {
		return prefix + "_deadline_expired"
	}
	return prefix + "_terminal"
}

func categoryOf(err error) string {
	var environment environmentError
	if errors.As(err, &environment) {
		return "environment"
	}
	return "harness"
}

func lastStage(clock *evidence.Clock) string {
	stages := clock.Stages()
	if len(stages) == 0 {
		return "validator_started"
	}
	return stages[len(stages)-1]["stage"].(string)
}

func execute(opts options, scenario contract.Scenario, identity *evidence.Identity, clock *evidence.Clock) (evidence.Document, error) {
	temporary, err := os.MkdirTemp("", "grokex-live-")
	if err != nil {
		return evidence.Document{}, environmentError{err}
	}
	defer os.RemoveAll(temporary)

	release, err := archive.Extract(opts.archive, filepath.Join(temporary, "artifact"))
	if err != nil {
		return evidence.Document{}, environmentError{err}
	}
	identity.Archive = release.Name
	identity.ArchiveSHA256 = release.SHA256
	clock.Mark("archive_extracted")

	home := filepath.Join(temporary, "home")
	workspace := filepath.Join(temporary, "workspace")
	for _, dir := range []string{home, workspace} {
		if err := os.MkdirAll(dir, 0o700); err != nil {
			return evidence.Document{}, environmentError{err}
		}
	}
	config, err := os.ReadFile(opts.config)
	if err != nil {
		return evidence.Document{}, environmentError{err}
	}
	if err := os.WriteFile(filepath.Join(home, "config.toml"), config, 0o600); err != nil {
		return evidence.Document{}, environmentError{err}
	}
	clock.Mark("fresh_home_prepared")

	ctx := context.Background()
	var tool *driver.DynamicTool
	if opts.scenario == scenarioContinuation {
		probe := oracle.ProbeTool
		tool = &probe
	}
	app, err := driver.Start(release.Binary(), home, workspace, release.Version(), tool)
	if err != nil {
		return evidence.Document{}, err
	}
	closed := false
	defer func() {
		if !closed {
			_ = app.Close()
		}
	}()
	clock.Mark("app_server_started")
	catalog, err := app.VerifyCatalog(ctx)
	if err != nil {
		return evidence.Document{}, err
	}
	clock.Mark("catalog_verified")

	var verdictFor func(*rollout.Graph) oracle.Verdict
	var lastRun driver.TurnRun
	switch opts.scenario {
	case scenarioBasic:
		clock.Mark("turn_submitted")
		run, err := app.StartThread(ctx, driver.TurnRequest{Prompt: oracle.BasicPrompt, Deadline: scenario.TurnDeadline()})
		if err != nil && !errors.Is(err, driver.ErrDeadline) {
			return evidence.Document{}, err
		}
		clock.Mark(terminalStage("turn", run))
		lastRun = run
		verdictFor = func(graph *rollout.Graph) oracle.Verdict { return oracle.Basic(graph, run) }
	case scenarioContinuation:
		clock.Mark("turn_submitted")
		first, err := app.StartThread(ctx, driver.TurnRequest{Prompt: oracle.ContinuationPrompt, Deadline: scenario.TurnDeadline()})
		if err != nil && !errors.Is(err, driver.ErrDeadline) {
			return evidence.Document{}, err
		}
		clock.Mark(terminalStage("turn", first))
		lastRun = first
		history := driver.TurnRun{ThreadID: first.ThreadID}
		if first.Completed() {
			clock.Mark("history_turn_submitted")
			history, err = app.ContinueThread(ctx, first.ThreadID, driver.TurnRequest{Prompt: oracle.HistoryPrompt, Deadline: scenario.TurnDeadline()})
			if err != nil && !errors.Is(err, driver.ErrDeadline) {
				return evidence.Document{}, err
			}
			clock.Mark(terminalStage("history_turn", history))
			lastRun = history
		}
		verdictFor = func(graph *rollout.Graph) oracle.Verdict {
			return oracle.Continuation(graph, first, history, app.Requests.ToolRequestCount())
		}
	case scenarioCollaboration:
		if catalog.MultiAgentVersion != "v2" {
			return evidence.Document{}, fmt.Errorf("catalog lists %s without multi-agent v2", driver.Model)
		}
		clock.Mark("turn_submitted")
		run, err := app.StartThread(ctx, driver.TurnRequest{Prompt: oracle.CollaborationPrompt, Effort: "ultra", Deadline: scenario.TurnDeadline()})
		if err != nil && !errors.Is(err, driver.ErrDeadline) {
			return evidence.Document{}, err
		}
		clock.Mark(terminalStage("turn", run))
		lastRun = run
		verdictFor = func(graph *rollout.Graph) oracle.Verdict { return oracle.Collaboration(graph, run, catalog) }
	case scenarioImage:
		clock.Mark("turn_submitted")
		generation, err := app.StartThread(ctx, driver.TurnRequest{Prompt: oracle.ImageGenerationPrompt, Deadline: scenario.TurnDeadline()})
		if err != nil && !errors.Is(err, driver.ErrDeadline) {
			return evidence.Document{}, err
		}
		clock.Mark(terminalStage("turn", generation))
		lastRun = generation
		edit := driver.TurnRun{ThreadID: generation.ThreadID}
		if generation.Completed() {
			clock.Mark("history_turn_submitted")
			edit, err = app.ContinueThread(ctx, generation.ThreadID, driver.TurnRequest{Prompt: oracle.ImageEditPrompt, Deadline: scenario.TurnDeadline()})
			if err != nil && !errors.Is(err, driver.ErrDeadline) {
				return evidence.Document{}, err
			}
			clock.Mark(terminalStage("history_turn", edit))
			lastRun = edit
		}
		verdictFor = func(graph *rollout.Graph) oracle.Verdict { return oracle.Image(graph, generation, edit) }
	default:
		return evidence.Document{}, fmt.Errorf("scenario %s has no canonical-session oracle", opts.scenario)
	}

	if lastRun.ThreadID != "" && lastRun.TurnID != "" && rollout.WaitForTurnComplete(home, lastRun.ThreadID, lastRun.TurnID, rolloutSettle) {
		clock.Mark("rollout_persisted")
	}
	if err := app.Close(); err != nil {
		return evidence.Document{}, fmt.Errorf("close app-server: %w", err)
	}
	closed = true
	clock.Mark("app_server_closed")

	graph, err := rollout.Scan(home)
	if err != nil {
		return evidence.Document{}, err
	}
	clock.Mark("sessions_scanned")
	verdict := verdictFor(graph)
	// Server requests the validator answered (approvals declined, the probe
	// tool answered) are part of every post-mortem.
	for key, value := range app.Requests.Diagnostics() {
		if _, taken := verdict.Diagnostics[key]; !taken {
			verdict.Diagnostics[key] = value
		}
	}
	clock.Mark("verdict")
	return evidence.FromVerdict(*identity, clock, verdict), nil
}
