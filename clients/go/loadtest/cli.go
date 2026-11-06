package loadtest

import (
	"errors"
	"flag"
	"fmt"
	"io"
	"slices"
	"strconv"
	"strings"
)

// walDirWarning is the one statement of the --wal-dir semantics: the flag is
// accepted for parity, ignored by the engine, and only its directory is
// removed by --reset.
const walDirWarning = "warning: the engine has no WAL directory (C ABI relate_database_open), so --wal-dir is ignored; a set --wal-dir is only removed by --reset"

// The two loadtest binaries share one command line: the same flag frame,
// the same common registrations and help rows, and the same
// parse/help/exit/run-error tail. This file states that plumbing once; each
// binary's main supplies only its name, summary, vocabulary, workload
// flags, and run/report pair. RunCLI is process-free: main supplies the
// arguments and error stream, and maps the returned code to os.Exit.

// flagKind is the closed vocabulary of a flag's value kind: the help value
// token and whether a rendered default is quoted both follow from it, so a
// constructor states the kind once and no two fields can disagree.
type flagKind int

const (
	flagBool flagKind = iota // no value token, no default suffix
	flagString
	flagInt
	flagFloat
)

func (k flagKind) token() string {
	switch k {
	case flagBool:
		return ""
	case flagString:
		return "string"
	case flagInt:
		return "int"
	case flagFloat:
		return "float"
	}
	panic(fmt.Sprintf("flagKind: unknown kind %d", k))
}

// defaultSuffix renders the help suffix for the registered default — the
// kind's one presentation policy: booleans show none, string defaults are
// quoted, numeric defaults are plain.
func (k flagKind) defaultSuffix(def string) string {
	switch k {
	case flagBool:
		return ""
	case flagString:
		return " (default " + strconv.Quote(def) + ")"
	default:
		return " (default " + def + ")"
	}
}

// CLIFlag declares one flag once: its name, its value kind, its help text,
// and its binding. The bind closure captures the same name the row renders,
// and the row derives the default itself from the registered FlagSet, so no
// fact is stated twice.
type CLIFlag struct {
	name string
	kind flagKind
	help string
	bind func(*flag.FlagSet, *Config)
}

// row renders the flag's help line from the registered FlagSet, whose
// DefValue is the one source of the default: a value flag with a non-empty
// default renders it (strings quoted), while a boolean or a defaultless
// flag renders no suffix. A row is rendered only for a bound flag (RunCLI
// binds the whole table before installing fs.Usage).
func (f CLIFlag) row(fs *flag.FlagSet) string {
	token := f.kind.token()
	text := "--" + f.name
	if token != "" {
		text += " " + token
	}
	help := f.help
	if def := fs.Lookup(f.name).DefValue; token != "" && def != "" {
		help += f.kind.defaultSuffix(def)
	}
	return fmt.Sprintf("  %-25s%s\n", text, help)
}

// valueFlag is the constructor shape every Config-backed flag uses: it
// resolves the target Config field at bind time and registers it under the
// captured name.
func valueFlag[T any](name string, kind flagKind, help string, target func(*Config) *T, set func(*flag.FlagSet, *T, string, T, string)) CLIFlag {
	return CLIFlag{name: name, kind: kind, help: help,
		bind: func(fs *flag.FlagSet, cfg *Config) {
			p := target(cfg)
			set(fs, p, name, *p, help)
		}}
}

func stringFlag(name, help string, target func(*Config) *string) CLIFlag {
	return valueFlag(name, flagString, help, target, (*flag.FlagSet).StringVar)
}

func intFlag(name, help string, target func(*Config) *int) CLIFlag {
	return valueFlag(name, flagInt, help, target, (*flag.FlagSet).IntVar)
}

func int64Flag(name, help string, target func(*Config) *int64) CLIFlag {
	return valueFlag(name, flagInt, help, target, (*flag.FlagSet).Int64Var)
}

func floatFlag(name, help string, target func(*Config) *float64) CLIFlag {
	return valueFlag(name, flagFloat, help, target, (*flag.FlagSet).Float64Var)
}

func boolFlag(name, help string, target func(*Config) *bool) CLIFlag {
	return valueFlag(name, flagBool, help, target, (*flag.FlagSet).BoolVar)
}

// groupThousands renders a non-negative integer with comma groups
// (10000 → "10,000"), the help text's spelling of the scale factor.
func groupThousands(n int) string {
	s := strconv.Itoa(n)
	if len(s) <= 3 {
		return s
	}
	groups := make([]string, 0, len(s)/3+1)
	for i := len(s); i > 0; i -= 3 {
		groups = append(groups, s[max(i-3, 0):i])
	}
	slices.Reverse(groups)
	return strings.Join(groups, ",")
}

// CLIVocabulary is the per-workload wording the shared help rows mention.
type CLIVocabulary struct {
	Graph     string // "fraud" / "identity": the synthetic graph's name
	ScaleUnit string // "accounts" / "users": what a scale of 1.0 generates
	PickNoun  string // "account" / "user": what the workload seed picks
}

// cliFlags composes one documented flag table: the shared flags, with the
// workload's own flags in their documented place between the seed flags and
// the run flags.
func cliFlags(v CLIVocabulary, workload []CLIFlag, help *bool) []CLIFlag {
	head := []CLIFlag{
		stringFlag("uri", "Database URI (:memory:, path, or file://)", func(c *Config) *string { return &c.URI }),
		floatFlag("scale", fmt.Sprintf("Graph size vs full generator (1.0 = %s %s)", groupThousands(entitiesPerScale), v.ScaleUnit), func(c *Config) *float64 { return &c.Scale }),
		int64Flag("graph-seed", fmt.Sprintf("RNG for the synthetic %s graph", v.Graph), func(c *Config) *int64 { return &c.GraphSeed }),
		int64Flag("workload-seed", fmt.Sprintf("RNG for use-case mix and %s picks", v.PickNoun), func(c *Config) *int64 { return &c.WorkloadSeed }),
	}
	tail := []CLIFlag{
		intFlag("ops", "Measured operations after warmup", func(c *Config) *int { return &c.Ops }),
		intFlag("warmup", "Unmeasured operations run first", func(c *Config) *int { return &c.Warmup }),
		stringFlag("out", "Report JSON path", func(c *Config) *string { return &c.Out }),
		stringFlag("wal-dir", "Accepted for flag parity; the engine has no WAL directory (a set --wal-dir is only removed by --reset)", func(c *Config) *string { return &c.WalDir }),
		boolFlag("reset", "Delete the existing database at --uri (and --wal-dir if set) before load", func(c *Config) *bool { return &c.Reset }),
		intFlag("workers", "Concurrent connections issuing ops (1 = serial)", func(c *Config) *int { return &c.Workers }),
		floatFlag("write-fraction", "Fraction of ops that are writes (0.0 = reads only, 1.0 = writes only)", func(c *Config) *float64 { return &c.WriteFraction }),
		stringFlag("logical-graph-size-limit", "Live graph + index cap (bytes or mb/mib/gb/gib; sets RELATE_LOGICAL_GRAPH_SIZE_LIMIT)", func(c *Config) *string { return &c.LogicalGraphSizeLimit }),
	}
	flags := make([]CLIFlag, 0, len(head)+len(workload)+len(tail)+1)
	flags = append(flags, head...)
	flags = append(flags, workload...)
	flags = append(flags, tail...)
	// The frame's own row: --help, bound to the frame's help target like
	// every other row binds a Config field.
	return append(flags, boolFlag("help", "Print this help and exit", func(*Config) *bool { return help }))
}

// CLI is one loadtest binary's command line: the shared frame plus the
// vocabulary and workload flag table that differ per workload.
type CLI[R any] struct {
	Name          string
	Summary       string
	Defaults      Config
	Vocabulary    CLIVocabulary
	WorkloadFlags []CLIFlag
	Run           func(Config) (R, error)
	Print         func(R, string, io.Writer)
}

// RunCLI owns the shared tail every loadtest binary runs (parse, help, the
// no-positionals contract, and the run) over the arguments main passes. It
// returns the process exit code (0 success/help, 1 run error, 2 usage
// error) and never exits the process; main maps it to os.Exit. The
// arguments and error stream are inputs, so the frame is testable
// in-process.
func RunCLI[R any](c CLI[R], args []string, stdout, stderr io.Writer) int {
	cfg := c.Defaults
	// The frame's whole flag table, stated once: the shared rows plus the
	// frame's own --help row, every CLIFlag binding a real target.
	help := false
	flags := cliFlags(c.Vocabulary, c.WorkloadFlags, &help)
	fs := flag.NewFlagSet(c.Name, flag.ContinueOnError)
	fs.SetOutput(stderr)
	for _, f := range flags {
		f.bind(fs, &cfg)
	}
	fs.Usage = func() {
		fmt.Fprintf(stderr, "%s\n\nUsage:\n  %s [flags]\n\nFlags:\n", c.Summary, c.Name)
		for _, f := range flags {
			fmt.Fprint(stderr, f.row(fs))
		}
	}
	if err := fs.Parse(args); err != nil {
		// The FlagSet already printed the offending flag (or the help) and
		// its usage block; only the exit code is left to map. ErrHelp is the
		// undefined -h alias (parseOne prints usage and returns it); the
		// documented --help flag exits 0 through the branch below.
		if errors.Is(err, flag.ErrHelp) {
			return 0
		}
		return 2
	}
	if help {
		fs.Usage()
		return 0
	}
	// The --wal-dir warning is flag interpretation, so the frame owns it and
	// writes it to the injected stream; the runner itself stays silent.
	if cfg.WalDir != "" {
		fmt.Fprintln(stderr, walDirWarning)
	}
	// The binaries take no positionals: name any unexpected operand loudly
	// instead of letting it pass silently.
	if fs.NArg() > 0 {
		fmt.Fprintf(stderr, "%s: unexpected positional argument(s): %s\n", c.Name, strings.Join(fs.Args(), " "))
		fs.Usage()
		return 2
	}
	r, err := c.Run(cfg)
	if err != nil {
		fmt.Fprintln(stderr, err)
		return 1
	}
	c.Print(r, cfg.Out, stdout)
	return 0
}
