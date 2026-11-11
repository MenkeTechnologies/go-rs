//! `gors::run_program`: stdout capture, stdin feeding, exit and panic
//! handling, and isolation between concurrent calls (goroutines included).

use gors::run_program;

#[test]
fn captures_fmt_print_and_os_stdout() {
    let src = r#"package main

import (
	"fmt"
	"os"
)

func main() {
	fmt.Println("hello", 42)
	fmt.Printf("%03d|", 7)
	os.Stdout.WriteString("raw\n")
}
"#;
    assert_eq!(run_program(src, "").unwrap(), "hello 42\n007|raw\n");
}

#[test]
fn bufio_scanner_and_fmt_scan_read_input() {
    let scanner = r#"package main

import (
	"bufio"
	"fmt"
	"os"
	"strings"
)

func main() {
	sc := bufio.NewScanner(os.Stdin)
	for sc.Scan() {
		fmt.Println(strings.ToUpper(sc.Text()))
	}
}
"#;
    assert_eq!(run_program(scanner, "ab\ncd").unwrap(), "AB\nCD\n");
    let scan = r#"package main

import "fmt"

func main() {
	var a, b int
	fmt.Scan(&a, &b)
	fmt.Println(a + b)
}
"#;
    assert_eq!(run_program(scan, "19 23\n").unwrap(), "42\n");
}

#[test]
fn errors_exit_and_panic() {
    assert!(run_program("package main\nfunc main() {", "").is_err());
    let exit = |code: i32| {
        format!(
            "package main\nimport (\n\t\"fmt\"\n\t\"os\"\n)\nfunc main() {{\n\tfmt.Println(\"a\")\n\tdefer fmt.Println(\"deferred\")\n\tos.Exit({code})\n\tfmt.Println(\"b\")\n}}\n"
        )
    };
    assert_eq!(run_program(&exit(0), "").unwrap(), "a\n");
    assert_eq!(run_program(&exit(3), "").unwrap_err(), "exit status 3");
    let panics = "package main\nfunc main() {\n\tvar m map[string]int\n\tm[\"k\"] = 1\n}\n";
    assert_eq!(
        run_program(panics, "").unwrap_err(),
        "panic: assignment to entry in nil map"
    );
    // `os.Exit` from a goroutine while `main` waits on it ends the program.
    let goroutine_exit = "package main\nimport (\n\t\"fmt\"\n\t\"os\"\n)\nfunc main() {\n\tdone := make(chan bool)\n\tgo func() {\n\t\tfmt.Println(\"bye\")\n\t\tos.Exit(4)\n\t}()\n\t<-done\n\tfmt.Println(\"unreached\")\n}\n";
    assert_eq!(
        run_program(goroutine_exit, "").unwrap_err(),
        "exit status 4"
    );
}

#[test]
fn concurrent_calls_with_goroutines_keep_their_own_output() {
    let handles: Vec<_> = (0..8)
        .map(|i| {
            std::thread::spawn(move || {
                let src = format!(
                    r#"package main

import (
	"bufio"
	"fmt"
	"os"
)

func main() {{
	in := bufio.NewReader(os.Stdin)
	tag, _ := in.ReadString('\n')
	done := make(chan bool)
	for g := 0; g < 3; g++ {{
		go func(g int) {{
			for k := 0; k < 50; k++ {{
				fmt.Printf("{i}:%s:%d:%d\n", tag[:len(tag)-1], g, k)
			}}
			done <- true
		}}(g)
	}}
	for g := 0; g < 3; g++ {{
		<-done
	}}
}}
"#
                );
                (i, run_program(&src, &format!("in{i}\n")).unwrap())
            })
        })
        .collect();
    for h in handles {
        let (i, out) = h.join().unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 150, "{out}");
        for g in 0..3 {
            let mine: Vec<&str> = lines
                .iter()
                .copied()
                .filter(|l| l.starts_with(&format!("{i}:in{i}:{g}:")))
                .collect();
            let want: Vec<String> = (0..50).map(|k| format!("{i}:in{i}:{g}:{k}")).collect();
            assert_eq!(mine, want);
        }
    }
}

#[test]
fn os_args_is_the_program_name_not_the_host_arguments() {
    let src = r#"package main

import (
	"fmt"
	"os"
)

func main() {
	fmt.Println(len(os.Args), os.Args[0])
}
"#;
    assert_eq!(run_program(src, "").unwrap(), "1 main\n");
}
