//! The feedback report the student may save (self-contained HTML, no scripts,
//! no external resources).

use crate::kb::KnowledgeBase;
use crate::pipeline::RunOutcome;
use crate::text::html_escape as esc;

pub fn render_html(outcome: &RunOutcome, kb: &KnowledgeBase, criteria: &[String], created_local: &str) -> String {
    let mut body = String::new();
    for cid in criteria {
        let name = kb.criterion_name(cid);
        let items: Vec<_> = outcome.items.iter().filter(|i| &i.criterion == cid).collect();
        body.push_str(&format!("<h2>{}</h2>\n", esc(&name)));
        if items.is_empty() {
            body.push_str("<p class=\"none\">No questions for this criterion in this run.</p>\n");
        }
        for it in items {
            body.push_str(&format!(
                "<div class=\"item\"><p class=\"check\">{}</p><blockquote>{}</blockquote><p class=\"q\">{}</p></div>\n",
                esc(&it.check_description),
                esc(&it.student_quote),
                esc(&it.question)
            ));
        }
    }
    let draft_notice = if kb.has_unreviewed_checks() {
        "<p class=\"warn\">The checklist behind these questions is a DRAFT that has not yet been reviewed by IB teachers.</p>"
    } else {
        ""
    };
    let models = outcome.models_responded.iter().cloned().collect::<Vec<_>>().join(", ");
    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Feedback questions</title>
<style>
body{{font-family:system-ui,Segoe UI,sans-serif;max-width:780px;margin:2rem auto;padding:0 1rem;color:#1d2330;line-height:1.5}}
h1{{font-size:1.5rem}} h2{{font-size:1.15rem;margin-top:2rem;border-bottom:1px solid #ccd;padding-bottom:.25rem}}
.item{{margin:1rem 0 1.5rem}} .check{{font-size:.85rem;color:#556;margin:0}}
blockquote{{margin:.4rem 0;padding:.4rem .8rem;border-left:3px solid #99a;background:#f4f5f8;font-style:italic}}
.q{{font-weight:600;margin:.3rem 0}} .meta,.none{{color:#556;font-size:.9rem}} .warn{{background:#fff4d6;padding:.5rem .8rem;border-radius:4px}}
</style></head><body>
<h1>Feedback questions: IB Physics Internal Assessment (draft)</h1>
<p class="meta">Created {created} · {shown} question(s) · model(s): {models} · knowledge base {kbv} ({year} syllabus)</p>
<p>These are questions to help you think about your own draft. They are not corrections and contain no suggested wording. Quoted passages are your own words, with personal details replaced by placeholders such as [STUDENT_NAME].</p>
{draft_notice}
{body}
</body></html>
"#,
        created = esc(created_local),
        shown = outcome.items.len(),
        models = esc(&models),
        kbv = esc(&kb.manifest.kb_version),
        year = kb.manifest.syllabus_year,
    )
}
