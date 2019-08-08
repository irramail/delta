use regex::Regex;

use unicode_segmentation::UnicodeSegmentation;

use crate::align;

/// Infer the edit operations responsible for the differences between a collection of old and new
/// lines. A "line" is a string. An annotated line is a Vec of (op, &str) pairs, where the &str
/// slices are slices of the line, and their concatenation equals the line. Return the input minus
/// and plus lines, in annotated form.
pub fn infer_edits<'a, EditOperation>(
    minus_lines: &'a Vec<String>,
    plus_lines: &'a Vec<String>,
    noop_deletion: EditOperation,
    deletion: EditOperation,
    noop_insertion: EditOperation,
    insertion: EditOperation,
    max_line_distance: f64,
) -> (
    Vec<Vec<(EditOperation, &'a str)>>, // annotated minus lines
    Vec<Vec<(EditOperation, &'a str)>>, // annotated plus lines
)
where
    EditOperation: Copy,
    EditOperation: PartialEq,
{
    let mut annotated_minus_lines = Vec::<Vec<(EditOperation, &str)>>::new();
    let mut annotated_plus_lines = Vec::<Vec<(EditOperation, &str)>>::new();

    let mut emitted = 0; // plus lines emitted so far

    'minus_lines_loop: for minus_line in minus_lines {
        let mut considered = 0; // plus lines considered so far as match for minus_line
        for plus_line in &plus_lines[emitted..] {
            let alignment = align::Alignment::new(tokenize(minus_line), tokenize(plus_line));
            let (annotated_minus_line, annotated_plus_line, distance) = annotate(
                alignment,
                noop_deletion,
                deletion,
                noop_insertion,
                insertion,
                minus_line,
                plus_line,
            );
            if distance <= max_line_distance {
                // minus_line and plus_line are inferred to be a homologous pair.

                // Emit as unpaired the plus lines already considered and rejected
                for plus_line in &plus_lines[emitted..(emitted + considered)] {
                    annotated_plus_lines.push(vec![(noop_insertion, plus_line)]);
                }
                emitted += considered;
                annotated_minus_lines.push(annotated_minus_line);
                annotated_plus_lines.push(annotated_plus_line);
                emitted += 1;

                // Greedy: move on to the next minus line.
                continue 'minus_lines_loop;
            } else {
                considered += 1;
            }
        }
        // No homolog was found for minus i; emit as unpaired.
        annotated_minus_lines.push(vec![(noop_deletion, minus_line)]);
    }
    // Emit any remaining plus lines
    for plus_line in &plus_lines[emitted..] {
        annotated_plus_lines.push(vec![(noop_insertion, plus_line)]);
    }

    (annotated_minus_lines, annotated_plus_lines)
}

/// Split line into tokens for alignment. The alignment algorithm aligns sequences of substrings;
/// not individual characters.
fn tokenize(line: &str) -> Vec<&str> {
    let separators = Regex::new(r"[ ,;.:()\[\]<>]+").unwrap();
    let mut tokens = Vec::new();
    let mut offset = 0;
    for m in separators.find_iter(line) {
        tokens.push(&line[offset..m.start()]);
        tokens.push(m.as_str());
        offset = m.end();
    }
    if offset < line.len() {
        tokens.push(&line[offset..line.len()]);
    }
    tokens
}

/// Use alignment to "annotate" minus and plus lines. An "annotated" line is a sequence of
/// (s: &str, a: Annotation) pairs, where the &strs reference the memory
/// of the original line and their concatenation equals the line.
fn annotate<'a, Annotation>(
    alignment: align::Alignment<'a>,
    noop_deletion: Annotation,
    deletion: Annotation,
    noop_insertion: Annotation,
    insertion: Annotation,
    minus_line: &'a str,
    plus_line: &'a str,
) -> (Vec<(Annotation, &'a str)>, Vec<(Annotation, &'a str)>, f64)
where
    Annotation: Copy,
{
    let mut annotated_minus_line = Vec::new();
    let mut annotated_plus_line = Vec::new();

    let (mut x_offset, mut y_offset) = (0, 0);
    let (mut minus_line_offset, mut plus_line_offset) = (0, 0);
    let (mut d_numer, mut d_denom) = (0, 0);

    // Note that the inputs to align::Alignment are not the original strings themselves, but
    // sequences of substrings derived from the tokenization process. We have just applied
    // run_length_encoding to "coalesce" runs of the same edit operation into a single
    // operation. We now need to form a &str, pointing into the memory of the original line,
    // identifying a "section" which is the concatenation of the substrings involved in this
    // coalesced operation. That's what the following closures do. Note that they must be called
    // once only since they advance offset pointers.
    let get_section = |n: usize,
                       line_offset: &mut usize,
                       substrings_offset: &mut usize,
                       substrings: &[&str],
                       line: &'a str| {
        let section_length = substrings[*substrings_offset..*substrings_offset + n]
            .iter()
            .fold(0, |n, s| n + s.len());
        let old_offset = *line_offset;
        *line_offset += section_length;
        *substrings_offset += n;
        &line[old_offset..*line_offset]
    };
    let mut minus_section = |n| {
        get_section(
            n,
            &mut minus_line_offset,
            &mut x_offset,
            &alignment.x,
            minus_line,
        )
    };
    let mut plus_section = |n| {
        get_section(
            n,
            &mut plus_line_offset,
            &mut y_offset,
            &alignment.y,
            plus_line,
        )
    };
    let distance_contribution = |section: &str| section.trim().graphemes(true).count();

    for (op, n) in alignment.coalesced_operations() {
        match op {
            align::Operation::Deletion => {
                let minus_section = minus_section(n);
                let n_d = distance_contribution(minus_section);
                d_denom += n_d;
                d_numer += n_d;
                annotated_minus_line.push((deletion, minus_section));
            }
            align::Operation::NoOp => {
                let minus_section = minus_section(n);
                let n_d = distance_contribution(minus_section);
                d_denom += n_d; // TODO 2x ?
                annotated_minus_line.push((noop_deletion, minus_section));
                annotated_plus_line.push((noop_insertion, plus_section(n)));
            }
            align::Operation::Substitution => {
                let minus_section = minus_section(n);
                let n_d = distance_contribution(minus_section);
                d_denom += n_d; // TODO 2x ?
                d_numer += n_d; // TODO 2x?
                annotated_minus_line.push((deletion, minus_section));
                annotated_plus_line.push((insertion, plus_section(n)));
            }
            align::Operation::Insertion => {
                let plus_section = plus_section(n);
                let n_d = distance_contribution(plus_section);
                d_denom += n_d;
                d_numer += n_d;
                annotated_plus_line.push((insertion, plus_section));
            }
        }
    }
    let distance = (d_numer as f64) / (d_denom as f64);
    (annotated_minus_line, annotated_plus_line, distance)
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_segmentation::UnicodeSegmentation;

    #[derive(Clone, Copy, Debug, PartialEq)]
    enum EditOperation {
        MinusNoop,
        PlusNoop,
        Deletion,
        Insertion,
    }

    type Annotation<'a> = (EditOperation, &'a str);
    type AnnotatedLine<'a> = Vec<Annotation<'a>>;
    type AnnotatedLines<'a> = Vec<AnnotatedLine<'a>>;
    type Edits<'a> = (AnnotatedLines<'a>, AnnotatedLines<'a>);

    use EditOperation::*;

    #[test]
    fn test_tokenize_1() {
        assert_eq!(tokenize("aaa bbb"), vec!["aaa", " ", "bbb"])
    }

    #[test]
    fn test_tokenize_2() {
        assert_eq!(
            tokenize("fn coalesce_edits<'a, EditOperation>("),
            vec![
                "fn",
                " ",
                "coalesce_edits",
                "<",
                "'a",
                ", ",
                "EditOperation",
                ">("
            ]
        );
    }

    #[test]
    fn test_tokenize_3() {
        assert_eq!(
            tokenize("fn coalesce_edits<'a, 'b, EditOperation>("),
            vec![
                "fn",
                " ",
                "coalesce_edits",
                "<",
                "'a",
                ", ",
                "'b",
                ", ",
                "EditOperation",
                ">("
            ]
        );
    }

    #[test]
    fn test_tokenize_4() {
        assert_eq!(
            tokenize("annotated_plus_lines.push(vec![(noop_insertion, plus_line)]);"),
            vec![
                "annotated_plus_lines",
                ".",
                "push",
                "(",
                "vec!",
                "[(",
                "noop_insertion",
                ", ",
                "plus_line",
                ")]);"
            ]
        );
    }

    #[test]
    fn test_infer_edits_1() {
        assert_paired_edits(
            vec!["aaa"],
            vec!["aba"],
            (
                vec![vec![(Deletion, "aaa")]],
                vec![vec![(Insertion, "aba")]],
            ),
        )
    }

    #[test]
    fn test_infer_edits_1_2() {
        assert_paired_edits(
            vec!["aaa ccc"],
            vec!["aba ccc"],
            (
                vec![vec![(Deletion, "aaa"), (MinusNoop, " ccc")]],
                vec![vec![(Insertion, "aba"), (PlusNoop, " ccc")]],
            ),
        )
    }

    #[test]
    fn test_infer_edits_2() {
        assert_paired_edits(
            vec!["áaa"],
            vec!["ááb"],
            (
                vec![vec![(Deletion, "áaa")]],
                vec![vec![(Insertion, "ááb")]],
            ),
        )
    }

    #[test]
    fn test_infer_edits_3() {
        assert_paired_edits(
            vec!["d.iteritems()"],
            vec!["d.items()"],
            (
                vec![vec![
                    (MinusNoop, "d."),
                    (Deletion, "iteritems"),
                    (MinusNoop, "()"),
                ]],
                vec![vec![
                    (PlusNoop, "d."),
                    (Insertion, "items"),
                    (PlusNoop, "()"),
                ]],
            ),
        )
    }

    #[test]
    fn test_infer_edits_4() {
        assert_edits(
            vec!["á a a á a a á a a", "á á b á á b á á b"],
            vec!["á á b á á c á á b"],
            (
                vec![
                    vec![(MinusNoop, "á a a á a a á a a")],
                    vec![
                        (MinusNoop, "á á b á á "),
                        (Deletion, "b"),
                        (MinusNoop, " á á b"),
                    ],
                ],
                vec![vec![
                    (PlusNoop, "á á b á á "),
                    (Insertion, "c"),
                    (PlusNoop, " á á b"),
                ]],
            ),
            0.66,
        )
    }

    #[test]
    fn test_infer_edits_5() {
        assert_edits(
            vec!["aaaa a aaa", "bbbb b bbb", "cccc c ccc"],
            vec!["bbbb ! bbb", "dddd d ddd", "cccc ! ccc"],
            (
                vec![
                    vec![(MinusNoop, "aaaa a aaa")],
                    vec![(MinusNoop, "bbbb "), (Deletion, "b"), (MinusNoop, " bbb")],
                    vec![(MinusNoop, "cccc "), (Deletion, "c"), (MinusNoop, " ccc")],
                ],
                vec![
                    vec![(PlusNoop, "bbbb "), (Insertion, "!"), (PlusNoop, " bbb")],
                    vec![(PlusNoop, "dddd d ddd")],
                    vec![(PlusNoop, "cccc "), (Insertion, "!"), (PlusNoop, " ccc")],
                ],
            ),
            0.66,
        )
    }

    #[test]
    fn test_infer_edits_6() {
        assert_no_edits(
            vec![
                "             let mut i = 0;",
                "             for ((_, c0), (_, c1)) in s0.zip(s1) {",
                "                 if c0 != c1 {",
                "                     break;",
                "                 } else {",
                "                     i += c0.len();",
                "                 }",
                "             }",
                "             i",
            ],
            vec![
                "             s0.zip(s1)",
                "                 .take_while(|((_, c0), (_, c1))| c0 == c1) // TODO: Don't consume one-past-the-end!",
                "                 .fold(0, |offset, ((_, c0), (_, _))| offset + c0.len())"
            ], 0.66)
    }

    #[test]
    fn test_infer_edits_6_1() {
        let (after, before) = (
            "                     i += c0.len();",
            "                 .fold(0, |offset, ((_, c0), (_, _))| offset + c0.len())",
        );
        println!("          before: {}", before);
        println!("          after : {}", after);
        println!("tokenized before: {:?}", tokenize(before));
        println!("tokenized after : {:?}", tokenize(after));
        println!(
            "distance: {:?}",
            align::Alignment::new(tokenize(before), tokenize(after)).distance_parts()
        );
    }

    #[test]
    fn test_infer_edits_7() {
        assert_edits(
            vec!["fn coalesce_edits<'a, EditOperation>("],
            vec!["fn coalesce_edits<'a, 'b, EditOperation>("],
            (
                vec![vec![
                    (MinusNoop, "fn coalesce_edits<'a"),
                    (MinusNoop, ", EditOperation>("),
                ]],
                vec![vec![
                    (PlusNoop, "fn coalesce_edits<'a"),
                    (Insertion, ", 'b"),
                    (PlusNoop, ", EditOperation>("),
                ]],
            ),
            0.66,
        )
    }

    fn assert_edits(
        minus_lines: Vec<&str>,
        plus_lines: Vec<&str>,
        expected_edits: Edits,
        max_line_distance: f64,
    ) {
        let minus_lines = minus_lines
            .into_iter()
            .map(|s| s.to_string())
            .collect::<Vec<String>>();
        let plus_lines = plus_lines
            .into_iter()
            .map(|s| s.to_string())
            .collect::<Vec<String>>();
        let actual_edits = infer_edits(
            &minus_lines,
            &plus_lines,
            MinusNoop,
            Deletion,
            PlusNoop,
            Insertion,
            max_line_distance,
        );
        assert_eq!(actual_edits, expected_edits);
    }

    // Assert that no edits are inferred for the supplied minus and plus lines.
    fn assert_no_edits(minus_lines: Vec<&str>, plus_lines: Vec<&str>, max_line_distance: f64) {
        let expected_edits = (
            minus_lines.iter().map(|s| vec![(MinusNoop, *s)]).collect(),
            plus_lines.iter().map(|s| vec![(PlusNoop, *s)]).collect(),
        );
        assert_edits(minus_lines, plus_lines, expected_edits, max_line_distance)
    }

    // Assertions for a single pair of lines, considered as a homologous pair. We set
    // max_line_distance = 1.0 in order that the pair will be inferred to be homologous.
    fn assert_paired_edits(minus_lines: Vec<&str>, plus_lines: Vec<&str>, expected_edits: Edits) {
        assert_consistent_pairs(&expected_edits);
        assert_edits(minus_lines, plus_lines, expected_edits, 1.0);
    }

    fn assert_consistent_pairs(edits: &Edits) {
        let (minus_annotated_lines, plus_annotated_lines) = edits;

        for (minus_annotated_line, plus_annotated_line) in
            minus_annotated_lines.iter().zip(plus_annotated_lines)
        {
            let (minus_total, minus_delta) = summarize_annotated_line(minus_annotated_line);
            let (plus_total, plus_delta) = summarize_annotated_line(plus_annotated_line);
            assert_eq!(
                minus_total - minus_delta,
                plus_total - plus_delta,
                "\nInconsistent edits:\n \
                 {:?}\n \
                 \tminus_total - minus_delta = {} - {} = {}\n \
                 {:?}\n \
                 \tplus_total  - plus_delta  = {} - {} = {}\n",
                minus_annotated_line,
                minus_total,
                minus_delta,
                minus_total - minus_delta,
                plus_annotated_line,
                plus_total,
                plus_delta,
                plus_total - plus_delta
            );
        }
    }

    fn summarize_annotated_line(sections: &AnnotatedLine) -> (usize, usize) {
        let mut total = 0;
        let mut delta = 0;
        for (edit, s) in sections {
            let length = s.graphemes(true).count();
            total += length;
            if is_edit(edit) {
                delta += length;
            }
        }
        (total, delta)
    }

    // For debugging test failures:

    #[allow(dead_code)]
    fn compare_annotated_lines(actual: Edits, expected: Edits) {
        let (minus, plus) = actual;
        println!("\n\nactual minus:");
        print_annotated_lines(minus);
        println!("\nactual plus:");
        print_annotated_lines(plus);

        let (minus, plus) = expected;
        println!("\n\nexpected minus:");
        print_annotated_lines(minus);
        println!("\nexpected plus:");
        print_annotated_lines(plus);
    }

    #[allow(dead_code)]
    fn print_annotated_lines(annotated_lines: AnnotatedLines) {
        for annotated_line in annotated_lines {
            print_annotated_line(annotated_line);
        }
    }

    #[allow(dead_code)]
    fn print_annotated_line(annotated_line: AnnotatedLine) {
        for (edit, s) in annotated_line {
            print!("({} {}), ", fmt_edit(edit), s.trim_end());
        }
        print!("\n");
    }

    #[allow(dead_code)]
    fn fmt_edit(edit: EditOperation) -> &'static str {
        match edit {
            MinusNoop => "MinusNoop",
            Deletion => "Deletion",
            PlusNoop => "PlusNoop",
            Insertion => "Insertion",
        }
    }

    fn is_edit(edit: &EditOperation) -> bool {
        *edit == Deletion || *edit == Insertion
    }

}