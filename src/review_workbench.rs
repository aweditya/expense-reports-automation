use std::collections::BTreeMap;

use crate::draft::EvidenceReference;
use crate::field_conventions::{FieldControl, FieldEntryMode};
use crate::ocr_grounding::{match_quote_to_region_id, DocumentOcrGroundingSummary};
use crate::review_packet::{
    CopyField, DocumentSnapshotCard, DocumentSnapshotField, FilingStatus, ReviewPacket,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct WorkbenchIndex {
    field_targets: BTreeMap<String, String>,
    instance_targets: BTreeMap<String, String>,
    section_targets: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GroundingLinkTarget {
    summary: DocumentOcrGroundingSummary,
    href: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ComparisonLinkTarget {
    summary: crate::DocumentOcrComparisonSummary,
    diff_href: Option<String>,
    inspection_href: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkbenchFieldOcrSignal {
    confidence: crate::ConfidenceLevel,
    status: crate::OcrComparisonStatus,
    grounded: bool,
    summary: String,
    diff_href: Option<String>,
    inspection_href: Option<String>,
    grounding_href: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OcrReviewQueueItem {
    path: String,
    label: String,
    target: String,
    signal: WorkbenchFieldOcrSignal,
}

pub fn render_review_workbench_html(packet: &ReviewPacket) -> String {
    let index = build_workbench_index(packet);
    let grounding_lookup = build_grounding_lookup(packet);
    let comparison_lookup = build_comparison_lookup(packet);
    let mut html = String::new();

    html.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    html.push_str(
        "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>Expense Report Review Workbench</title>\n",
    );
    html.push_str("<style>\n");
    html.push_str(include_str!("review_workbench.css"));
    html.push_str("\n</style>\n");
    html.push_str(
        "<script>\nconst reviewSessionState={currentDraftVersionId:null,saveInFlight:false};\nfunction fieldControl(card){return card?card.querySelector('.field-control'):null;}\nfunction issueCountLabel(count){return count===1?'1 change pending':`${count} changes pending`;}\nfunction openAncestorDetails(target){let current=target?.parentElement;while(current){if(current.tagName==='DETAILS'){current.open=true;}current=current.parentElement;}}\nfunction revealHashTarget(){const rawHash=window.location.hash||'';if(!rawHash||rawHash==='#'){return;}const target=document.getElementById(rawHash.slice(1));if(!target){return;}openAncestorDetails(target);target.scrollIntoView({behavior:'smooth',block:'center'});const focusTarget=target.matches('.field-card')?target.querySelector('.field-control, .structured-row-input'):target.querySelector?.('.field-control, .structured-row-input');if(focusTarget){focusTarget.focus();if(typeof focusTarget.select==='function'){focusTarget.select();}}}\nfunction jumpToField(event,targetId){const target=document.getElementById(targetId);if(!target){return;}event.preventDefault();openAncestorDetails(target);target.scrollIntoView({behavior:'smooth',block:'center'});window.location.hash=targetId;const focusTarget=target.querySelector('.field-control, .structured-row-input');if(focusTarget){focusTarget.focus();if(typeof focusTarget.select==='function'){focusTarget.select();}}}\nfunction flashCopy(button){button.textContent='Copied';setTimeout(()=>{button.textContent='Copy';},900);}\nfunction structuredRowsPayload(editor){if(!editor){return [];}const rows=[];for(const row of editor.querySelectorAll('.structured-row')){const values={};let hasAnyValue=false;for(const input of row.querySelectorAll('.structured-row-input')){const key=input.dataset.columnKey||'';let value=('value' in input)?input.value:'';if(input.dataset.columnControl==='checkbox'){value=input.value;}if(value!==''&&value!==null){hasAnyValue=true;}values[key]=value;}if(hasAnyValue){rows.push(values);}}return rows;}\nfunction valueForCopy(card){const control=fieldControl(card);if(!control){return '';}if(control.classList.contains('structured-list-editor')){const rows=structuredRowsPayload(control);if(rows.length===0){return '';}return rows.map((row,index)=>`${index+1}. `+Object.entries(row).filter(([,value])=>value!==''&&value!==null).map(([key,value])=>`${key}: ${value}`).join(' | ')).join('\\n');}if(control.tagName==='SELECT'){return control.value||'';}if('value' in control){return control.value||'';}return '';} \nfunction copyFieldValue(button){const card=button.closest('.field-card');if(!card){return;}const value=valueForCopy(card);if(!value){return;}navigator.clipboard.writeText(value);flashCopy(button);} \nfunction parseJsonData(value,fallback){if(!value){return fallback;}try{return JSON.parse(value);}catch(_err){return fallback;}}\nfunction normalizeFieldValue(control){if(!control){return null;}if(control.classList.contains('structured-list-editor')){const columns=parseJsonData(control.dataset.columnsJson,'[]');const rows=structuredRowsPayload(control).map((row)=>{const obj={};for(const column of columns){const raw=row[column.key]??'';if(raw===''){continue;}if(column.control==='checkbox'){obj[column.key]=raw==='true';}else{obj[column.key]=raw;}}return obj;});return rows.length===0?[]:rows;}if(control.dataset.control==='checkbox'){if(control.value===''){return null;}return control.value==='true';}if('value' in control){return control.value===''?null:control.value;}return null;}\nfunction initialFieldValue(control){if(!control){return null;}const fallback=control.classList.contains('structured-list-editor')?[]:null;return parseJsonData(control.dataset.initialJson,fallback);} \nfunction valuesEqual(left,right){return JSON.stringify(left)===JSON.stringify(right);} \nfunction fieldReasonSelect(card){return card.querySelector('.field-reason-select');}\nfunction fieldNoteInput(card){return card.querySelector('.field-note-input');}\nfunction fieldConfirmInput(card){return card.querySelector('.field-confirm-input');}\nfunction updateFieldDirtyState(card){const control=fieldControl(card);if(!control){return;}const edited=!valuesEqual(initialFieldValue(control),normalizeFieldValue(control));const confirmed=Boolean(fieldConfirmInput(card)?.checked);card.classList.toggle('dirty',edited||confirmed);const badge=card.querySelector('.field-dirty-badge');if(badge){badge.hidden=!(edited||confirmed);}}\nfunction refreshDirtySummary(){const dirtyCards=[...document.querySelectorAll('.field-card.dirty')];const counter=document.getElementById('pending-change-count');if(counter){counter.textContent=issueCountLabel(dirtyCards.length);}const saveButton=document.getElementById('save-review-button');const resetButton=document.getElementById('reset-review-button');if(saveButton){saveButton.disabled=reviewSessionState.saveInFlight||dirtyCards.length===0;}if(resetButton){resetButton.disabled=reviewSessionState.saveInFlight||dirtyCards.length===0;}}\nfunction syncCardStateFromEventTarget(target){const card=target.closest('.field-card');if(!card){return;}updateFieldDirtyState(card);refreshDirtySummary();}\nfunction structuredRowTemplate(editor,rowValues){const columns=parseJsonData(editor.dataset.columnsJson,[]);const row=document.createElement('div');row.className='structured-row';for(const column of columns){const cell=document.createElement('label');cell.className='structured-cell';const label=document.createElement('span');label.className='structured-cell-label';label.textContent=column.label;cell.appendChild(label);let input;if(column.control==='select'){input=document.createElement('select');const blank=document.createElement('option');blank.value='';blank.textContent='';input.appendChild(blank);for(const optionValue of column.allowed_values||[]){const option=document.createElement('option');option.value=optionValue;option.textContent=optionValue.replaceAll('_',' ');input.appendChild(option);}}else if(column.control==='checkbox'){input=document.createElement('select');[['','Unset'],['true','Yes'],['false','No']].forEach(([value,labelText])=>{const option=document.createElement('option');option.value=value;option.textContent=labelText;input.appendChild(option);});}else if(column.control==='date'){input=document.createElement('input');input.type='date';}else{input=document.createElement(column.control==='textarea'?'textarea':'input');if(input.tagName==='INPUT'){input.type='text';if(column.control==='currency'||column.control==='number'){input.inputMode='decimal';}}}input.className='structured-row-input';input.dataset.columnKey=column.key;input.dataset.columnControl=column.control;input.value=(rowValues&&rowValues[column.key])||'';input.addEventListener('input',()=>syncCardStateFromEventTarget(input));input.addEventListener('change',()=>syncCardStateFromEventTarget(input));cell.appendChild(input);row.appendChild(cell);}const removeButton=document.createElement('button');removeButton.type='button';removeButton.className='structured-row-remove';removeButton.textContent='Remove row';removeButton.addEventListener('click',()=>{row.remove();syncCardStateFromEventTarget(editor);});row.appendChild(removeButton);return row;}\nfunction addStructuredListRow(button){const editor=button.closest('.structured-list-editor');if(!editor){return;}const rows=editor.querySelector('.structured-list-rows');if(!rows){return;}rows.appendChild(structuredRowTemplate(editor,{}));syncCardStateFromEventTarget(editor);} \nfunction resetReviewForm(){for(const card of document.querySelectorAll('.field-card')){const control=fieldControl(card);if(!control){continue;}const initial=initialFieldValue(control);if(control.classList.contains('structured-list-editor')){const rows=control.querySelector('.structured-list-rows');if(rows){rows.innerHTML='';for(const rowValues of Array.isArray(initial)?initial:[]){rows.appendChild(structuredRowTemplate(control,rowValues));}}}else if(control.dataset.control==='checkbox'){control.value=initial===null?'':String(initial);}else if('value' in control){control.value=initial??'';}const reason=fieldReasonSelect(card);const note=fieldNoteInput(card);const confirm=fieldConfirmInput(card);if(reason){reason.value='';}if(note){note.value='';}if(confirm){confirm.checked=false;}updateFieldDirtyState(card);}setWorkbenchStatus('Unsaved review edits cleared.','neutral');refreshDirtySummary();revealHashTarget();}\nasync function loadReviewSessionSummary(){try{const response=await fetch('review-session',{headers:{'Accept':'application/json'}});if(!response.ok){throw new Error(`session lookup failed (${response.status})`);}const payload=await response.json();reviewSessionState.currentDraftVersionId=payload.current_draft_version_id;const versionLabel=document.getElementById('current-draft-version');if(versionLabel){versionLabel.textContent=`v${payload.current_draft_version_id}`;}const filingStatus=document.getElementById('current-filing-status');if(filingStatus){filingStatus.textContent=payload.filing_status.replaceAll('_',' ');} }catch(err){setWorkbenchStatus(`Unable to load review session metadata: ${err.message}`,'error');}}\nfunction buildRevisionPayload(){const fieldEdits=[];const confirmedReviewPaths=[];const annotations=[];for(const card of document.querySelectorAll('.field-card')){const control=fieldControl(card);if(!control||card.classList.contains('readonly')){if(fieldConfirmInput(card)?.checked){confirmedReviewPaths.push(card.dataset.fieldPath||'');}continue;}const current=normalizeFieldValue(control);const initial=initialFieldValue(control);const changed=!valuesEqual(current,initial);const path=card.dataset.fieldPath||control.dataset.fieldPath||'';const reason=fieldReasonSelect(card)?.value||'';const note=(fieldNoteInput(card)?.value||'').trim();if(changed){fieldEdits.push({path,value:current,reason:reason||null,note:note||null,origin:'local_app.review_workbench'});if(reason){annotations.push({path,reason,note:note||null});}}if(fieldConfirmInput(card)?.checked){confirmedReviewPaths.push(path);}}\nreturn {base_version_id:reviewSessionState.currentDraftVersionId,actor_role:'financial_administrator',label:'FA saved revision',field_edits:fieldEdits,confirmed_review_paths:[...new Set(confirmedReviewPaths.filter(Boolean))],annotations};}\nfunction setWorkbenchStatus(message,tone){const target=document.getElementById('workbench-status');if(!target){return;}target.textContent=message;target.dataset.tone=tone;}\nasync function saveReviewChanges(){if(reviewSessionState.saveInFlight){return;}const payload=buildRevisionPayload();if(payload.field_edits.length===0&&payload.confirmed_review_paths.length===0){setWorkbenchStatus('No review changes to save.','neutral');return;}reviewSessionState.saveInFlight=true;setWorkbenchStatus('Saving review changes and recomputing readiness…','saving');refreshDirtySummary();try{const response=await fetch('review-session/save',{method:'POST',headers:{'Content-Type':'application/json','Accept':'application/json'},body:JSON.stringify(payload)});const result=await response.json();if(!response.ok){throw new Error(result.error||`save failed (${response.status})`);}sessionStorage.setItem('reviewWorkbenchFlash',`Saved review revision v${result.version_id}. Readiness recomputed.`);window.location.reload();}catch(err){setWorkbenchStatus(`Save failed: ${err.message}`,'error');reviewSessionState.saveInFlight=false;refreshDirtySummary();}}\nfunction initializeReviewWorkbench(){for(const control of document.querySelectorAll('.field-control, .structured-row-input')){control.addEventListener('input',()=>syncCardStateFromEventTarget(control));control.addEventListener('change',()=>syncCardStateFromEventTarget(control));}\nfor(const editor of document.querySelectorAll('.structured-list-editor')){const rows=editor.querySelector('.structured-list-rows');const initial=parseJsonData(editor.dataset.initialJson,[]);if(rows&&rows.children.length===0&&Array.isArray(initial)){for(const rowValues of initial){rows.appendChild(structuredRowTemplate(editor,rowValues));}}}\nfor(const card of document.querySelectorAll('.field-card')){updateFieldDirtyState(card);}refreshDirtySummary();loadReviewSessionSummary();const flash=sessionStorage.getItem('reviewWorkbenchFlash');if(flash){setWorkbenchStatus(flash,'success');sessionStorage.removeItem('reviewWorkbenchFlash');}const saveButton=document.getElementById('save-review-button');const resetButton=document.getElementById('reset-review-button');const refreshButton=document.getElementById('reload-review-button');if(saveButton){saveButton.addEventListener('click',saveReviewChanges);}if(resetButton){resetButton.addEventListener('click',resetReviewForm);}if(refreshButton){refreshButton.addEventListener('click',()=>window.location.reload());}revealHashTarget();window.addEventListener('hashchange',revealHashTarget);}\nfunction hasDirtyFields(){return document.querySelectorAll('.field-card.dirty').length>0;}\nwindow.addEventListener('beforeunload',function(event){if(hasDirtyFields()&&!reviewSessionState.saveInFlight){event.preventDefault();event.returnValue='';}});\ndocument.addEventListener('keydown',function(event){if((event.metaKey||event.ctrlKey)&&event.key==='s'){event.preventDefault();saveReviewChanges();}});\ndocument.addEventListener('DOMContentLoaded',initializeReviewWorkbench);\n</script>\n",
    );
    html.push_str("</head>\n<body>\n<div class=\"shell\">\n");

    render_header(&mut html, packet);
    render_toolbar(&mut html);
    render_document_snapshot_panel(&mut html, packet);
    html.push_str("<main class=\"workbench-grid\">\n");
    render_issues_panel(
        &mut html,
        packet,
        &index,
        &grounding_lookup,
        &comparison_lookup,
    );
    render_copy_panel(
        &mut html,
        packet,
        &index,
        &grounding_lookup,
        &comparison_lookup,
    );
    html.push_str("</main>\n");
    render_attachments_panel(&mut html, packet);
    html.push_str("</div>\n</body>\n</html>\n");
    html
}

fn render_header(html: &mut String, packet: &ReviewPacket) {
    html.push_str("<header class=\"hero\">\n");
    html.push_str("<div class=\"hero-copy\">\n");
    html.push_str("<p class=\"eyebrow\">Expense Report Review</p>\n");
    html.push_str("<h1>FA Workbench</h1>\n");
    html.push_str("<p class=\"hero-status\">");
    html.push_str(&escape_html(filing_status_label(
        packet.summary.filing_status,
    )));
    html.push_str("</p>\n");
    html.push_str("<p class=\"hero-subtitle\">");
    html.push_str(&escape_html(
        packet
            .summary
            .payee_name
            .as_deref()
            .unwrap_or("Unknown payee"),
    ));
    html.push_str(" · ");
    html.push_str(&escape_html(
        packet
            .summary
            .event_name
            .as_deref()
            .unwrap_or("Missing event name"),
    ));
    html.push_str("</p>\n");
    html.push_str("</div>\n");
    html.push_str("<div class=\"hero-cards\">\n");
    summary_card(
        html,
        "Trip Window",
        packet.summary.trip_window.as_deref().unwrap_or("[missing]"),
    );
    summary_card(
        html,
        "Report Total USD",
        packet
            .summary
            .report_total_usd
            .as_deref()
            .unwrap_or("[missing]"),
    );
    summary_card(
        html,
        "Readiness",
        &format!(
            "{} automation · {} user input · {} review",
            packet.summary.readiness.automation_gap_count,
            packet.summary.readiness.user_input_gap_count,
            packet.summary.readiness.manual_review_count
        ),
    );
    summary_card(
        html,
        "Confidence",
        &format!(
            "{} high · {} medium · {} low",
            packet.summary.confidence.high,
            packet.summary.confidence.medium,
            packet.summary.confidence.low
        ),
    );
    html.push_str("</div>\n</header>\n");
}

fn render_toolbar(html: &mut String) {
    html.push_str("<section class=\"workbench-toolbar panel\">\n");
    html.push_str("<div class=\"toolbar-main\">");
    html.push_str("<div><p class=\"eyebrow\">Review Session</p><h2>Editable Filing Surface</h2>");
    html.push_str("<p class=\"toolbar-meta\">Current draft <span id=\"current-draft-version\">v?</span> · filing status <span id=\"current-filing-status\">loading…</span></p>");
    html.push_str("</div>");
    html.push_str("<div class=\"toolbar-actions\">");
    html.push_str("<button class=\"primary-button\" id=\"save-review-button\" type=\"button\">Save And Recompute</button>");
    html.push_str("<button class=\"secondary-button\" id=\"reset-review-button\" type=\"button\">Reset Unsaved Changes</button>");
    html.push_str("<button class=\"ghost-button\" id=\"reload-review-button\" type=\"button\">Reload Latest Review State</button>");
    html.push_str("</div></div>");
    html.push_str("<div class=\"toolbar-subrow\">");
    html.push_str("<p class=\"toolbar-status\" id=\"workbench-status\" data-tone=\"neutral\">Edit machine-filled values, supply missing fields, then save to create a new reviewed draft version.</p>");
    html.push_str("<p class=\"toolbar-count\" id=\"pending-change-count\">0 changes pending</p>");
    html.push_str("</div>");
    html.push_str("<div class=\"toolbar-links\">");
    html.push_str("<a href=\"overview\">Bundle Overview</a>");
    html.push_str(
        "<a href=\"artifact/draft.yaml\" target=\"_blank\" rel=\"noreferrer\">Draft YAML</a>",
    );
    html.push_str("<a href=\"artifact/review_packet.json\" target=\"_blank\" rel=\"noreferrer\">Review Packet JSON</a>");
    html.push_str(
        "<a href=\"artifact/ledger.json\" target=\"_blank\" rel=\"noreferrer\">Ledger JSON</a>",
    );
    html.push_str(
        "<a href=\"review-session\" target=\"_blank\" rel=\"noreferrer\">Session Summary</a>",
    );
    html.push_str("<a href=\"manifest\" target=\"_blank\" rel=\"noreferrer\">Bundle Manifest</a>");
    html.push_str("</div>");
    html.push_str("</section>\n");
}

fn render_issues_panel(
    html: &mut String,
    packet: &ReviewPacket,
    index: &WorkbenchIndex,
    grounding_lookup: &BTreeMap<String, GroundingLinkTarget>,
    comparison_lookup: &BTreeMap<String, ComparisonLinkTarget>,
) {
    html.push_str("<section class=\"panel issues-panel\">\n");
    html.push_str(
        "<div class=\"panel-heading\"><p class=\"eyebrow\">Queue</p><h2>Issues</h2></div>\n",
    );
    if packet.issues_queue.is_empty() {
        html.push_str("<p class=\"empty-state\">No blocking or review issues.</p>\n");
    } else {
        html.push_str("<ul class=\"issue-list\">\n");
        for issue in &packet.issues_queue {
            html.push_str("<li class=\"issue-card ");
            html.push_str(issue_class_name(issue.class));
            html.push_str("\">\n");
            html.push_str("<div class=\"issue-topline\">");
            html.push_str("<span class=\"issue-class\">");
            html.push_str(&escape_html(issue_class_name(issue.class)));
            html.push_str("</span>");
            if let Some(source) = issue.source.as_deref() {
                html.push_str("<span class=\"issue-source\">");
                html.push_str(&escape_html(source));
                html.push_str("</span>");
            }
            html.push_str("</div>\n");
            html.push_str("<h3>");
            html.push_str(&escape_html(&issue.label));
            html.push_str("</h3>\n");
            html.push_str("<p class=\"issue-path\">");
            html.push_str(&escape_html(&issue.path));
            html.push_str("</p>\n");
            if let Some(value) = issue.current_value.as_deref() {
                html.push_str("<p class=\"issue-value\">Current value: ");
                html.push_str(&escape_html(value));
                html.push_str("</p>\n");
            }
            html.push_str("<p class=\"issue-message\">");
            html.push_str(&escape_html(&issue.message));
            html.push_str("</p>\n");
            if let Some(target) = issue_target(issue.path.as_str(), index) {
                html.push_str("<a class=\"issue-link\" href=\"#");
                html.push_str(&escape_html(&target));
                html.push_str("\" onclick=\"jumpToField(event, '");
                html.push_str(&escape_html_attribute(&target));
                html.push_str("')\">Jump to field</a>\n");
            }
            html.push_str("</li>\n");
        }
        html.push_str("</ul>\n");
    }
    let ocr_review_items =
        collect_ocr_review_items(packet, index, grounding_lookup, comparison_lookup);
    if !ocr_review_items.is_empty() {
        html.push_str(
            "<div class=\"issues-subsection\"><p class=\"eyebrow\">OCR Review</p><h3>Fields To Double-Check</h3><ul class=\"issue-list ocr-review-list\">",
        );
        for item in ocr_review_items {
            html.push_str("<li class=\"issue-card ocr-review-card ");
            html.push_str(ocr_status_class(item.signal.status));
            html.push_str("\">");
            html.push_str("<div class=\"issue-topline\">");
            html.push_str("<span class=\"issue-class\">");
            html.push_str(ocr_status_label(item.signal.status));
            html.push_str("</span>");
            html.push_str("<span class=\"issue-source\">ocr ");
            html.push_str(confidence_level_label(item.signal.confidence));
            html.push_str("</span>");
            if item.signal.grounded {
                html.push_str("<span class=\"issue-source\">grounded</span>");
            }
            html.push_str("</div>");
            html.push_str("<h3>");
            html.push_str(&escape_html(&item.label));
            html.push_str("</h3>");
            html.push_str("<p class=\"issue-path\">");
            html.push_str(&escape_html(&item.path));
            html.push_str("</p>");
            html.push_str("<p class=\"issue-message\">");
            html.push_str(&escape_html(&item.signal.summary));
            html.push_str("</p>");
            html.push_str("<div class=\"ocr-review-links\">");
            html.push_str("<a class=\"issue-link\" href=\"#");
            html.push_str(&escape_html(&item.target));
            html.push_str("\" onclick=\"jumpToField(event, '");
            html.push_str(&escape_html_attribute(&item.target));
            html.push_str("')\">Jump to field</a>");
            if let Some(href) = item.signal.diff_href.as_deref() {
                html.push_str("<a class=\"issue-link\" href=\"");
                html.push_str(&escape_html_attribute(href));
                html.push_str("\" target=\"_blank\" rel=\"noreferrer noopener\">Open OCR diff</a>");
            }
            if let Some(href) = item.signal.grounding_href.as_deref() {
                html.push_str("<a class=\"issue-link\" href=\"");
                html.push_str(&escape_html_attribute(href));
                html.push_str(
                    "\" target=\"_blank\" rel=\"noreferrer noopener\">Open grounded source</a>",
                );
            }
            html.push_str("</div>");
            html.push_str("</li>");
        }
        html.push_str("</ul></div>");
    }
    html.push_str("</section>\n");
}

fn render_copy_panel(
    html: &mut String,
    packet: &ReviewPacket,
    index: &WorkbenchIndex,
    grounding_lookup: &BTreeMap<String, GroundingLinkTarget>,
    comparison_lookup: &BTreeMap<String, ComparisonLinkTarget>,
) {
    html.push_str("<section class=\"panel copy-panel\">\n");
    html.push_str("<div class=\"panel-heading\"><p class=\"eyebrow\">Oracle Copy View</p><h2>Ready-To-Copy Fields</h2></div>\n");
    for section in &packet.copy_sections {
        let section_id = anchor_id("section", &section.key);
        html.push_str("<section class=\"copy-section\" id=\"");
        html.push_str(&escape_html(&section_id));
        html.push_str("\">\n");
        html.push_str("<h3>");
        html.push_str(&escape_html(&section.label));
        html.push_str("</h3>\n");
        for instance in &section.instances {
            let instance_id = anchor_id("instance", &instance.path);
            html.push_str("<article class=\"copy-instance\" id=\"");
            html.push_str(&escape_html(&instance_id));
            html.push_str("\">\n");
            if section.repeated {
                html.push_str("<h4>");
                html.push_str(&escape_html(&instance.label));
                html.push_str("</h4>\n");
            }
            html.push_str("<div class=\"field-list\">\n");
            for field in &instance.fields {
                render_copy_field(html, field, index, grounding_lookup, comparison_lookup);
            }
            html.push_str("</div>\n</article>\n");
        }
        html.push_str("</section>\n");
    }
    html.push_str("</section>\n");
}

fn render_copy_field(
    html: &mut String,
    field: &CopyField,
    index: &WorkbenchIndex,
    grounding_lookup: &BTreeMap<String, GroundingLinkTarget>,
    comparison_lookup: &BTreeMap<String, ComparisonLinkTarget>,
) {
    let ocr_signal = derive_field_ocr_signal(field, comparison_lookup, grounding_lookup);
    let field_id = index
        .field_targets
        .get(&field.path)
        .cloned()
        .unwrap_or_else(|| anchor_id("field", &field.path));
    let input_id = format!("{field_id}-input");
    html.push_str("<article class=\"field-card");
    if field.needs_review {
        html.push_str(" needs-review");
    }
    if !field.present {
        html.push_str(" missing");
    }
    if field_is_readonly(field) {
        html.push_str(" readonly");
    } else {
        html.push_str(" editable");
    }
    if field.control == FieldControl::StructuredList {
        html.push_str(" structured");
    }
    html.push_str("\" id=\"");
    html.push_str(&escape_html(&field_id));
    html.push_str("\" data-field-path=\"");
    html.push_str(&escape_html_attribute(&field.path));
    html.push_str("\">\n");
    html.push_str("<div class=\"field-head\">\n");
    html.push_str("<div><p class=\"field-label\">");
    html.push_str(&escape_html(&field.label));
    html.push_str("</p><p class=\"field-path\">");
    html.push_str(&escape_html(&field.path));
    html.push_str("</p></div>\n");
    html.push_str("<div class=\"field-badges\">");
    html.push_str("<span class=\"badge field-dirty-badge\" hidden>edited</span>");
    if let Some(source) = field.source.as_deref() {
        badge(html, source);
    }
    badge(html, field.entry_mode.as_str());
    if field.required {
        badge(html, "required");
    }
    if field.needs_review {
        badge(html, "review");
    }
    if let Some(signal) = ocr_signal.as_ref() {
        html.push_str("<span class=\"badge ocr-field-status ");
        html.push_str(ocr_status_class(signal.status));
        html.push_str("\">ocr ");
        html.push_str(ocr_status_label(signal.status));
        html.push_str("</span>");
    }
    html.push_str("</div>\n</div>\n");
    html.push_str("<div class=\"field-body\">\n");
    render_field_editor(html, field, &input_id);
    html.push_str("<div class=\"field-actions\">");
    html.push_str("<button class=\"copy-button\" type=\"button\" onclick=\"copyFieldValue(this)\">Copy</button>");
    html.push_str("</div>\n</div>\n");
    html.push_str("<p class=\"field-guidance\">");
    html.push_str(&escape_html(field_guidance(field)));
    html.push_str("</p>\n");
    if let Some(signal) = ocr_signal.as_ref() {
        render_field_ocr_signal(html, signal);
    }
    render_review_controls(html, field);
    render_inline_evidence(html, field, grounding_lookup);
    html.push_str("</article>\n");
}

fn render_field_editor(html: &mut String, field: &CopyField, input_id: &str) {
    let value = field.value.as_deref().unwrap_or("");
    let placeholder = if field.present {
        ""
    } else {
        field_placeholder(field)
    };
    let readonly = if field_is_readonly(field) {
        " readonly"
    } else {
        ""
    };
    let disabled = if field_is_readonly(field) {
        " disabled"
    } else {
        ""
    };

    html.push_str("<div class=\"field-editor\">");
    html.push_str("<label class=\"field-editor-label\" for=\"");
    html.push_str(&escape_html(input_id));
    html.push_str("\">");
    html.push_str(if field_is_readonly(field) {
        "Review computed value"
    } else if field.present {
        "Review or edit value"
    } else {
        "Enter missing value"
    });
    html.push_str("</label>");

    if field.control == FieldControl::StructuredList {
        render_structured_list_editor(html, field, input_id);
    } else if field.control == FieldControl::Textarea {
        html.push_str("<textarea class=\"field-input field-control\" id=\"");
        html.push_str(&escape_html(input_id));
        html.push_str("\" data-control=\"textarea\" data-initial-json=\"");
        html.push_str(&escape_html_attribute(&scalar_initial_json(value)));
        html.push_str("\"");
        html.push_str(" data-field-path=\"");
        html.push_str(&escape_html_attribute(&field.path));
        html.push_str("\" placeholder=\"");
        html.push_str(&escape_html_attribute(placeholder));
        html.push_str("\"");
        html.push_str(readonly);
        html.push_str(">");
        html.push_str(&escape_html(value));
        html.push_str("</textarea>");
    } else if field.control == FieldControl::Select {
        html.push_str("<select class=\"field-input field-control\" id=\"");
        html.push_str(&escape_html(input_id));
        html.push_str("\" data-control=\"select\" data-field-path=\"");
        html.push_str(&escape_html_attribute(&field.path));
        html.push_str("\" data-initial-json=\"");
        html.push_str(&escape_html_attribute(&scalar_initial_json(value)));
        html.push_str("\"");
        html.push_str(disabled);
        html.push_str(">");
        html.push_str("<option value=\"\"></option>");
        for option in &field.allowed_values {
            html.push_str("<option value=\"");
            html.push_str(&escape_html_attribute(option));
            html.push_str("\"");
            if option == value {
                html.push_str(" selected");
            }
            html.push_str(">");
            html.push_str(&escape_html(&humanize_machine_label(option)));
            html.push_str("</option>");
        }
        html.push_str("</select>");
    } else if field.control == FieldControl::Checkbox {
        html.push_str("<select class=\"field-input field-control checkbox-select\" id=\"");
        html.push_str(&escape_html(input_id));
        html.push_str("\" data-control=\"checkbox\" data-field-path=\"");
        html.push_str(&escape_html_attribute(&field.path));
        html.push_str("\" data-initial-json=\"");
        html.push_str(&escape_html_attribute(&bool_initial_json(
            field.value.as_deref(),
        )));
        html.push_str("\"");
        html.push_str(disabled);
        html.push_str(">");
        render_checkbox_option(html, "", "Unset", value.is_empty());
        render_checkbox_option(html, "true", "Yes", value == "true");
        render_checkbox_option(html, "false", "No", value == "false");
        html.push_str("</select>");
    } else {
        let input_type = if field.control == FieldControl::Date {
            "date"
        } else {
            "text"
        };
        html.push_str("<input class=\"field-input field-control\" id=\"");
        html.push_str(&escape_html(input_id));
        html.push_str("\" type=\"");
        html.push_str(input_type);
        html.push_str("\" data-control=\"");
        html.push_str(field.control.as_str());
        html.push_str("\" data-field-path=\"");
        html.push_str(&escape_html_attribute(&field.path));
        html.push_str("\" data-initial-json=\"");
        html.push_str(&escape_html_attribute(&scalar_initial_json(value)));
        html.push_str("\" value=\"");
        html.push_str(&escape_html_attribute(value));
        html.push_str("\" placeholder=\"");
        html.push_str(&escape_html_attribute(placeholder));
        html.push_str("\"");
        if field.control.uses_decimal_input_mode() {
            html.push_str(" inputmode=\"decimal\"");
        }
        html.push_str(readonly);
        html.push_str(">");
    }
    html.push_str("</div>");
}

fn render_structured_list_editor(html: &mut String, field: &CopyField, input_id: &str) {
    html.push_str("<div class=\"structured-list-editor field-control\" id=\"");
    html.push_str(&escape_html(input_id));
    html.push_str("\" data-control=\"structured_list\" data-field-path=\"");
    html.push_str(&escape_html_attribute(&field.path));
    html.push_str("\" data-columns-json=\"");
    html.push_str(&escape_html_attribute(
        &serde_json::to_string(&field.collection_columns).expect("columns should serialize"),
    ));
    html.push_str("\" data-initial-json=\"");
    html.push_str(&escape_html_attribute(
        &serde_json::to_string(&field.collection_rows).expect("rows should serialize"),
    ));
    html.push_str("\">");
    html.push_str("<div class=\"structured-list-head\">");
    html.push_str("<p class=\"structured-list-copy\">");
    html.push_str(if field.present {
        "Edit the repeated rows directly. Add or remove rows as needed."
    } else {
        "Add one or more rows to supply this missing repeated field."
    });
    html.push_str("</p>");
    html.push_str("</div>");
    html.push_str("<div class=\"structured-list-rows\">");
    for row in &field.collection_rows {
        render_structured_list_row(html, &field.collection_columns, &row.values);
    }
    html.push_str("</div>");
    if !field_is_readonly(field) {
        html.push_str("<div class=\"structured-list-actions\"><button class=\"secondary-button\" type=\"button\" onclick=\"addStructuredListRow(this)\">Add row</button></div>");
    }
    html.push_str("</div>");
}

fn render_structured_list_row(
    html: &mut String,
    columns: &[crate::review_packet::CopyCollectionColumn],
    values: &BTreeMap<String, String>,
) {
    html.push_str("<div class=\"structured-row\">");
    for column in columns {
        html.push_str("<label class=\"structured-cell\">");
        html.push_str("<span class=\"structured-cell-label\">");
        html.push_str(&escape_html(&column.label));
        html.push_str("</span>");
        render_collection_column_control(
            html,
            column,
            values.get(&column.key).map(String::as_str).unwrap_or(""),
        );
        html.push_str("</label>");
    }
    html.push_str("<button class=\"structured-row-remove\" type=\"button\" onclick=\"this.closest('.structured-row').remove();syncCardStateFromEventTarget(this);\">Remove row</button>");
    html.push_str("</div>");
}

fn render_collection_column_control(
    html: &mut String,
    column: &crate::review_packet::CopyCollectionColumn,
    value: &str,
) {
    match column.control {
        FieldControl::Select => {
            html.push_str("<select class=\"structured-row-input\" data-column-key=\"");
            html.push_str(&escape_html_attribute(&column.key));
            html.push_str("\" data-column-control=\"select\">");
            html.push_str("<option value=\"\"></option>");
            for option in &column.allowed_values {
                html.push_str("<option value=\"");
                html.push_str(&escape_html_attribute(option));
                html.push_str("\"");
                if option == value {
                    html.push_str(" selected");
                }
                html.push_str(">");
                html.push_str(&escape_html(&humanize_machine_label(option)));
                html.push_str("</option>");
            }
            html.push_str("</select>");
        }
        FieldControl::Checkbox => {
            html.push_str(
                "<select class=\"structured-row-input checkbox-select\" data-column-key=\"",
            );
            html.push_str(&escape_html_attribute(&column.key));
            html.push_str("\" data-column-control=\"checkbox\">");
            render_checkbox_option(html, "", "Unset", value.is_empty());
            render_checkbox_option(html, "true", "Yes", value == "true");
            render_checkbox_option(html, "false", "No", value == "false");
            html.push_str("</select>");
        }
        FieldControl::Date => {
            html.push_str("<input class=\"structured-row-input\" type=\"date\" data-column-key=\"");
            html.push_str(&escape_html_attribute(&column.key));
            html.push_str("\" data-column-control=\"date\" value=\"");
            html.push_str(&escape_html_attribute(value));
            html.push_str("\">");
        }
        _ => {
            html.push_str("<input class=\"structured-row-input\" type=\"text\" data-column-key=\"");
            html.push_str(&escape_html_attribute(&column.key));
            html.push_str("\" data-column-control=\"");
            html.push_str(column.control.as_str());
            html.push_str("\" value=\"");
            html.push_str(&escape_html_attribute(value));
            html.push_str("\"");
            if column.control.uses_decimal_input_mode() {
                html.push_str(" inputmode=\"decimal\"");
            }
            html.push_str(">");
        }
    }
}

fn render_checkbox_option(html: &mut String, value: &str, label: &str, selected: bool) {
    html.push_str("<option value=\"");
    html.push_str(&escape_html_attribute(value));
    html.push_str("\"");
    if selected {
        html.push_str(" selected");
    }
    html.push_str(">");
    html.push_str(label);
    html.push_str("</option>");
}

fn render_review_controls(html: &mut String, field: &CopyField) {
    html.push_str("<details class=\"field-review-controls\">");
    html.push_str("<summary>Review controls</summary>");
    html.push_str("<div class=\"field-review-grid\">");
    html.push_str("<label class=\"review-confirm-toggle\"><input class=\"field-confirm-input\" type=\"checkbox\" onchange=\"syncCardStateFromEventTarget(this)\"> Mark this field reviewed</label>");
    if !field_is_readonly(field) {
        html.push_str("<label class=\"field-review-label\">Correction reason<select class=\"field-reason-select\" onchange=\"syncCardStateFromEventTarget(this)\">");
        html.push_str("<option value=\"\"></option>");
        for (value, label) in correction_reason_options() {
            html.push_str("<option value=\"");
            html.push_str(value);
            html.push_str("\">");
            html.push_str(label);
            html.push_str("</option>");
        }
        html.push_str("</select></label>");
        html.push_str("<label class=\"field-review-label review-note-label\">Review note<textarea class=\"field-note-input\" placeholder=\"Optional note for the ledger and feedback history\" oninput=\"syncCardStateFromEventTarget(this)\"></textarea></label>");
    }
    html.push_str("</div></details>");
}

fn render_inline_evidence(
    html: &mut String,
    field: &CopyField,
    grounding_lookup: &BTreeMap<String, GroundingLinkTarget>,
) {
    if field.evidence.is_empty() {
        return;
    }

    let details_id = anchor_id("field-evidence", &field.path);
    html.push_str("<details class=\"field-evidence\" id=\"");
    html.push_str(&escape_html(&details_id));
    html.push_str("\"><summary>");
    html.push_str(&escape_html(&format!(
        "Evidence ({})",
        field.evidence.len()
    )));
    html.push_str("</summary><div class=\"field-evidence-list\">");
    for evidence in &field.evidence {
        html.push_str("<article class=\"evidence-inline-card\" id=\"");
        html.push_str(&escape_html(&evidence_anchor_id(evidence)));
        html.push_str("\">");
        html.push_str("<div class=\"evidence-topline\">");
        badge(html, evidence_kind_display(evidence));
        if let Some(page_label) = evidence.page.map(|page| format!("page {page}")) {
            badge(html, &page_label);
        }
        html.push_str("</div>");
        html.push_str("<h4>");
        html.push_str(&escape_html(&evidence_title(evidence)));
        html.push_str("</h4>");
        if let Some(source_label) = evidence_source_label(evidence).as_deref() {
            html.push_str(
                "<p class=\"evidence-detail\"><span class=\"evidence-detail-label\">Source</span>",
            );
            html.push_str(&escape_html(source_label));
            html.push_str("</p>");
        }
        if let Some(origin_label) = evidence.origin.as_deref() {
            html.push_str(
                "<p class=\"evidence-detail\"><span class=\"evidence-detail-label\">Origin</span>",
            );
            html.push_str(&escape_html(origin_label));
            html.push_str("</p>");
        }
        if let Some(quote) = evidence.quote.as_deref() {
            html.push_str("<p class=\"evidence-quote-label\">Excerpt</p>");
            html.push_str("<blockquote>");
            html.push_str(&escape_html(quote));
            html.push_str("</blockquote>");
        }
        if let Some(document_href) = evidence_document_href(evidence) {
            html.push_str("<div class=\"evidence-document-actions\">");
            html.push_str("<a class=\"document-link\" href=\"");
            html.push_str(&escape_html_attribute(&document_href));
            html.push_str("\" target=\"_blank\" rel=\"noreferrer noopener\">");
            html.push_str("Open source document");
            if let Some(page) = evidence.page {
                html.push_str(" (page ");
                html.push_str(&escape_html(&page.to_string()));
                html.push(')');
            }
            html.push_str("</a>");
            if let Some((grounded_href, link_label)) =
                evidence_grounding_link(evidence, grounding_lookup)
            {
                html.push_str("<a class=\"document-link\" href=\"");
                html.push_str(&escape_html_attribute(&grounded_href));
                html.push_str("\" target=\"_blank\" rel=\"noreferrer noopener\">");
                html.push_str(link_label);
                html.push_str("</a>");
            }
            html.push_str("</div>");
        }
        html.push_str("</article>");
    }
    html.push_str("</div></details>");
}

fn render_document_snapshot_panel(html: &mut String, packet: &ReviewPacket) {
    html.push_str("<section class=\"panel document-snapshot-panel\">\n");
    html.push_str("<div class=\"panel-heading\"><p class=\"eyebrow\">Source Documents</p><h2>OCR And Extraction Snapshot</h2></div>\n");
    html.push_str("<p class=\"document-snapshot-copy\">Each card shows what the system recovered from the uploaded source document, even when the document could not yet be projected into a filing line.</p>");
    if packet.document_snapshots.is_empty() {
        html.push_str(
            "<p class=\"empty-state\">No source-document extraction snapshot is available.</p>\n",
        );
    } else {
        html.push_str("<div class=\"document-snapshot-grid\">");
        for document in &packet.document_snapshots {
            render_document_snapshot_card(html, document);
        }
        html.push_str("</div>");
    }
    html.push_str("</section>\n");
}

fn render_document_snapshot_card(html: &mut String, document: &DocumentSnapshotCard) {
    html.push_str("<article class=\"document-snapshot-card\">");
    html.push_str("<div class=\"document-snapshot-topline\">");
    badge(html, &humanize_machine_label(&document.kind));
    badge(html, &humanize_machine_label(&document.extraction_status));
    if document.projected_to_filing {
        badge(html, "projected");
    } else if document.used_in_bundle {
        badge(html, "bundle only");
    } else {
        badge(html, "not mapped");
    }
    html.push_str("</div>");
    html.push_str("<h3>");
    html.push_str(&escape_html(&document.filename));
    html.push_str("</h3>");
    html.push_str("<p class=\"document-snapshot-status\">");
    html.push_str(&escape_html(&document.status_label));
    html.push_str("</p>");
    if !document.summary_fields.is_empty() {
        html.push_str("<dl class=\"document-snapshot-fields\">");
        for field in &document.summary_fields {
            render_document_snapshot_field(html, field);
        }
        html.push_str("</dl>");
    }
    if !document.issue_messages.is_empty() {
        html.push_str("<ul class=\"document-snapshot-issues\">");
        for issue in &document.issue_messages {
            html.push_str("<li>");
            html.push_str(&escape_html(issue));
            html.push_str("</li>");
        }
        html.push_str("</ul>");
    }
    if let Some(comparison) = document.ocr_comparison.as_ref() {
        html.push_str("<div class=\"document-snapshot-ocr-summary\">");
        html.push_str("<p class=\"document-snapshot-ocr-heading\">OCR cross-check</p>");
        html.push_str("<p class=\"document-snapshot-ocr-meta\">");
        html.push_str(&escape_html(&format!(
            "{} pass{} compared · {} disagreement{} · {} confidence",
            comparison.compared_pass_count,
            if comparison.compared_pass_count == 1 {
                ""
            } else {
                "es"
            },
            comparison.disagreement_count,
            if comparison.disagreement_count == 1 {
                ""
            } else {
                "s"
            },
            confidence_level_label(comparison.overall_confidence)
        )));
        html.push_str("</p>");
        if !comparison.divergent_fields.is_empty() {
            html.push_str("<p class=\"document-snapshot-ocr-fields\">Disagreed fields: ");
            html.push_str(&escape_html(
                &comparison
                    .divergent_fields
                    .iter()
                    .map(|field| humanize_machine_label(field))
                    .collect::<Vec<_>>()
                    .join(", "),
            ));
            html.push_str("</p>");
        }
        if comparison.consistency_warning_count > 0 {
            html.push_str("<p class=\"document-snapshot-ocr-fields\">Consistency warnings: ");
            html.push_str(&escape_html(
                &comparison.consistency_warning_count.to_string(),
            ));
            html.push_str("</p>");
        }
        if !comparison.consistency_notes.is_empty() {
            html.push_str("<ul class=\"document-snapshot-ocr-notes\">");
            for note in &comparison.consistency_notes {
                html.push_str("<li>");
                html.push_str(&escape_html(note));
                html.push_str("</li>");
            }
            html.push_str("</ul>");
        }
        html.push_str("</div>");
    }
    if let Some(grounding) = document.ocr_grounding.as_ref() {
        html.push_str("<div class=\"document-snapshot-ocr-summary\">");
        html.push_str("<p class=\"document-snapshot-ocr-heading\">Grounded OCR</p>");
        html.push_str("<p class=\"document-snapshot-ocr-meta\">");
        html.push_str(&escape_html(&format!(
            "{} region{} localized via {}",
            grounding.regions.len(),
            if grounding.regions.len() == 1 {
                ""
            } else {
                "s"
            },
            grounding.geometry_source.as_str()
        )));
        html.push_str("</p>");
        if let Some(variant) = grounding.grounding_preprocess_variant {
            html.push_str("<p class=\"document-snapshot-ocr-fields\">Recovered via ");
            html.push_str(&escape_html(variant.as_str()));
            html.push_str(" preprocessing</p>");
        }
        html.push_str("</div>");
    }
    let document_href = format!("document/{}/{}", document.document_id, document.filename);
    let inspection_href = format!(
        "artifact/ocr_inspection/{}/inspection.html",
        document.document_id
    );
    html.push_str("<div class=\"document-snapshot-actions\">");
    html.push_str("<a class=\"document-link\" href=\"");
    html.push_str(&escape_html_attribute(&document_href));
    html.push_str("\" target=\"_blank\" rel=\"noreferrer noopener\">Open source document</a>");
    html.push_str("<a class=\"document-link\" href=\"");
    html.push_str(&escape_html_attribute(&inspection_href));
    html.push_str("\" target=\"_blank\" rel=\"noreferrer noopener\">Open OCR inspection</a>");
    if let Some(href) = document.ocr_comparison_href.as_ref() {
        html.push_str("<a class=\"document-link\" href=\"");
        html.push_str(&escape_html_attribute(href));
        html.push_str("\" target=\"_blank\" rel=\"noreferrer noopener\">Open OCR diff</a>");
    }
    if let Some(href) = document.ocr_grounding_href.as_ref() {
        html.push_str("<a class=\"document-link\" href=\"");
        html.push_str(&escape_html_attribute(href));
        html.push_str("\" target=\"_blank\" rel=\"noreferrer noopener\">Open grounded source</a>");
    }
    html.push_str("</div>");
    html.push_str("</article>");
}

fn render_document_snapshot_field(html: &mut String, field: &DocumentSnapshotField) {
    html.push_str("<div class=\"document-snapshot-field\">");
    html.push_str("<dt>");
    html.push_str("<span>");
    html.push_str(&escape_html(&field.label));
    html.push_str("</span>");
    if field.ocr_confidence.is_some() || field.ocr_status.is_some() || field.grounded {
        html.push_str("<span class=\"document-snapshot-signal-badges\">");
        if let Some(status) = field.ocr_status {
            html.push_str("<span class=\"badge document-snapshot-signal status ");
            html.push_str(ocr_status_class(status));
            html.push_str("\">");
            html.push_str(ocr_status_label(status));
            html.push_str("</span>");
        }
        if let Some(confidence) = field.ocr_confidence {
            html.push_str("<span class=\"badge document-snapshot-signal ");
            html.push_str(confidence_level_class(confidence));
            html.push_str("\">ocr ");
            html.push_str(confidence_level_label(confidence));
            html.push_str("</span>");
        }
        if field.grounded {
            html.push_str(
                "<span class=\"badge document-snapshot-signal grounded\">grounded</span>",
            );
        }
        html.push_str("</span>");
    }
    html.push_str("</dt>");
    html.push_str("<dd>");
    html.push_str(&escape_html(&field.value));
    html.push_str("</dd>");
    html.push_str("</div>");
}

fn field_is_readonly(field: &CopyField) -> bool {
    field.entry_mode == FieldEntryMode::ComputedReadonly
}

fn field_guidance(field: &CopyField) -> &'static str {
    if field_is_readonly(field) {
        "System-computed field. Review the supporting evidence, but edit this only if the downstream filing flow requires a manual override."
    } else if field.present {
        "Machine-filled value. Adjust it directly here if the parsed value is incomplete or incorrect."
    } else {
        "This field is currently missing. Enter the value here so the FA can continue the filing workflow."
    }
}

fn field_placeholder(field: &CopyField) -> &str {
    if field.required {
        "Required value"
    } else {
        "Optional value"
    }
}

fn evidence_document_href(evidence: &EvidenceReference) -> Option<String> {
    let document_id = evidence.document_id.as_deref()?;
    let filename = evidence
        .filename
        .as_deref()
        .or(evidence.document_id.as_deref())?;
    Some(format!("document/{document_id}/{filename}"))
}

fn build_grounding_lookup(packet: &ReviewPacket) -> BTreeMap<String, GroundingLinkTarget> {
    packet
        .document_snapshots
        .iter()
        .filter_map(|snapshot| {
            let summary = snapshot.ocr_grounding.clone()?;
            let href = snapshot
                .ocr_grounding_href
                .clone()
                .or_else(|| summary.preview_href.clone())?;
            Some((
                snapshot.document_id.clone(),
                GroundingLinkTarget { summary, href },
            ))
        })
        .collect()
}

fn build_comparison_lookup(packet: &ReviewPacket) -> BTreeMap<String, ComparisonLinkTarget> {
    packet
        .document_snapshots
        .iter()
        .filter_map(|snapshot| {
            let summary = snapshot.ocr_comparison.clone()?;
            Some((
                snapshot.document_id.clone(),
                ComparisonLinkTarget {
                    summary,
                    diff_href: snapshot.ocr_comparison_href.clone(),
                    inspection_href: format!(
                        "artifact/ocr_inspection/{}/inspection.html",
                        snapshot.document_id
                    ),
                },
            ))
        })
        .collect()
}

fn collect_ocr_review_items(
    packet: &ReviewPacket,
    index: &WorkbenchIndex,
    grounding_lookup: &BTreeMap<String, GroundingLinkTarget>,
    comparison_lookup: &BTreeMap<String, ComparisonLinkTarget>,
) -> Vec<OcrReviewQueueItem> {
    let mut items = Vec::new();
    for section in &packet.copy_sections {
        for instance in &section.instances {
            for field in &instance.fields {
                let Some(signal) =
                    derive_field_ocr_signal(field, comparison_lookup, grounding_lookup)
                else {
                    continue;
                };
                if !field_needs_ocr_review(&signal) {
                    continue;
                }
                let target = index
                    .field_targets
                    .get(&field.path)
                    .cloned()
                    .unwrap_or_else(|| anchor_id("field", &field.path));
                items.push(OcrReviewQueueItem {
                    path: field.path.clone(),
                    label: field.label.clone(),
                    target,
                    signal,
                });
            }
        }
    }
    items.sort_by_key(|item| {
        (
            std::cmp::Reverse(ocr_status_rank(item.signal.status)),
            confidence_level_rank(item.signal.confidence),
            item.label.clone(),
        )
    });
    items
}

fn field_needs_ocr_review(signal: &WorkbenchFieldOcrSignal) -> bool {
    signal.status != crate::OcrComparisonStatus::Consensus
        || signal.confidence != crate::ConfidenceLevel::High
}

fn derive_field_ocr_signal(
    field: &CopyField,
    comparison_lookup: &BTreeMap<String, ComparisonLinkTarget>,
    grounding_lookup: &BTreeMap<String, GroundingLinkTarget>,
) -> Option<WorkbenchFieldOcrSignal> {
    let mut statuses = Vec::new();
    let mut confidences = Vec::new();
    let mut grounded = false;
    let mut diff_href = None;
    let mut inspection_href = None;
    let mut grounding_href = None;
    let mut consistency_warning = false;
    let mut matched_labels = Vec::new();
    let mut saw_any = false;

    for evidence in &field.evidence {
        let Some(document_id) = evidence.document_id.as_deref() else {
            continue;
        };
        let Some(comparison_target) = comparison_lookup.get(document_id) else {
            continue;
        };
        saw_any = true;
        if diff_href.is_none() {
            diff_href = comparison_target.diff_href.clone();
        }
        if inspection_href.is_none() {
            inspection_href = Some(comparison_target.inspection_href.clone());
        }
        consistency_warning |= comparison_target.summary.consistency_warning_count > 0;

        let matched_regions = evidence
            .quote
            .as_deref()
            .and_then(|quote| {
                grounding_lookup
                    .get(document_id)
                    .and_then(|target| match_quote_to_region_id(&target.summary, quote))
            })
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();

        if grounding_href.is_none() {
            grounding_href =
                evidence_grounding_link(evidence, grounding_lookup).map(|(href, _)| href);
        }

        let matched_fields = matched_comparison_fields(
            field,
            evidence,
            &comparison_target.summary,
            &matched_regions,
        );
        if matched_fields.is_empty() {
            confidences.push(comparison_target.summary.overall_confidence);
            statuses.push(if comparison_target.summary.disagreement_count > 0 {
                crate::OcrComparisonStatus::Divergent
            } else if comparison_target.summary.overall_confidence == crate::ConfidenceLevel::High {
                crate::OcrComparisonStatus::Consensus
            } else {
                crate::OcrComparisonStatus::PartialConsensus
            });
            continue;
        }

        for matched in matched_fields {
            if !matched_labels.contains(&matched.field) {
                matched_labels.push(matched.field.clone());
            }
            confidences.push(matched.confidence);
            statuses.push(matched.status);
            grounded |= matched_regions
                .iter()
                .any(|region_id| region_id == &matched.field);
        }
    }

    if !saw_any {
        return None;
    }

    let status = statuses
        .into_iter()
        .max_by_key(|value| ocr_status_rank(*value))
        .unwrap_or(crate::OcrComparisonStatus::PartialConsensus);
    let confidence = confidences
        .into_iter()
        .min_by_key(|value| confidence_level_rank(*value))
        .unwrap_or(crate::ConfidenceLevel::Medium);
    let summary =
        workbench_field_ocr_summary(status, grounded, consistency_warning, &matched_labels);

    Some(WorkbenchFieldOcrSignal {
        confidence,
        status,
        grounded,
        summary,
        diff_href,
        inspection_href,
        grounding_href,
    })
}

fn matched_comparison_fields<'a>(
    field: &CopyField,
    evidence: &EvidenceReference,
    summary: &'a crate::DocumentOcrComparisonSummary,
    matched_regions: &[String],
) -> Vec<&'a crate::OcrFieldComparisonSummary> {
    let quote_value = evidence.quote.as_deref();
    let field_value = field.value.as_deref();
    summary
        .field_summaries
        .iter()
        .filter(|candidate| {
            matched_regions
                .iter()
                .any(|region_id| region_id == &candidate.field)
                || quote_value.is_some_and(|quote| {
                    comparison_value_matches(quote, candidate.consensus_value.as_deref())
                })
                || field_value.is_some_and(|value| {
                    comparison_value_matches(value, candidate.consensus_value.as_deref())
                })
        })
        .collect()
}

fn comparison_value_matches(value: &str, consensus_value: Option<&str>) -> bool {
    let Some(consensus_value) = consensus_value else {
        return false;
    };
    let normalized_value = normalize_comparison_text(value);
    let normalized_consensus = normalize_comparison_text(consensus_value);
    !normalized_value.is_empty()
        && !normalized_consensus.is_empty()
        && (normalized_value == normalized_consensus
            || normalized_value.contains(&normalized_consensus)
            || normalized_consensus.contains(&normalized_value))
}

fn normalize_comparison_text(value: &str) -> String {
    value
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect()
}

fn workbench_field_ocr_summary(
    status: crate::OcrComparisonStatus,
    grounded: bool,
    consistency_warning: bool,
    matched_fields: &[String],
) -> String {
    let mut summary = match status {
        crate::OcrComparisonStatus::Consensus => {
            if grounded {
                "Two OCR passes agreed on the linked evidence and localized it in the source document."
                    .to_owned()
            } else {
                "Two OCR passes agreed on the linked evidence, but source grounding is not available for this field."
                    .to_owned()
            }
        }
        crate::OcrComparisonStatus::PartialConsensus | crate::OcrComparisonStatus::Missing => {
            "OCR evidence is partially recovered across passes. Review the linked source before trusting this field."
                .to_owned()
        }
        crate::OcrComparisonStatus::Divergent => {
            "OCR passes disagreed on the linked evidence. This field needs review."
                .to_owned()
        }
    };
    if consistency_warning {
        summary.push_str(" The linked receipt also failed an internal amount-consistency check.");
    }
    if !matched_fields.is_empty() {
        summary.push_str(" Signals came from: ");
        summary.push_str(
            &matched_fields
                .iter()
                .map(|field| humanize_machine_label(field))
                .collect::<Vec<_>>()
                .join(", "),
        );
        summary.push('.');
    }
    summary
}

fn render_field_ocr_signal(html: &mut String, signal: &WorkbenchFieldOcrSignal) {
    html.push_str("<div class=\"field-ocr-signal\">");
    html.push_str("<div class=\"field-ocr-topline\">");
    html.push_str("<span class=\"badge ocr-field-status ");
    html.push_str(ocr_status_class(signal.status));
    html.push_str("\">");
    html.push_str(ocr_status_label(signal.status));
    html.push_str("</span>");
    html.push_str("<span class=\"badge ocr-field-confidence ");
    html.push_str(confidence_level_class(signal.confidence));
    html.push_str("\">ocr ");
    html.push_str(confidence_level_label(signal.confidence));
    html.push_str("</span>");
    if signal.grounded {
        html.push_str("<span class=\"badge ocr-field-grounded\">grounded</span>");
    }
    html.push_str("</div>");
    html.push_str("<p class=\"field-ocr-summary\">");
    html.push_str(&escape_html(&signal.summary));
    html.push_str("</p>");
    if signal.diff_href.is_some()
        || signal.inspection_href.is_some()
        || signal.grounding_href.is_some()
    {
        html.push_str("<div class=\"field-ocr-links\">");
        if let Some(href) = signal.diff_href.as_deref() {
            html.push_str("<a class=\"document-link\" href=\"");
            html.push_str(&escape_html_attribute(href));
            html.push_str("\" target=\"_blank\" rel=\"noreferrer noopener\">Open OCR diff</a>");
        }
        if let Some(href) = signal.inspection_href.as_deref() {
            html.push_str("<a class=\"document-link\" href=\"");
            html.push_str(&escape_html_attribute(href));
            html.push_str(
                "\" target=\"_blank\" rel=\"noreferrer noopener\">Open OCR inspection</a>",
            );
        }
        if let Some(href) = signal.grounding_href.as_deref() {
            html.push_str("<a class=\"document-link\" href=\"");
            html.push_str(&escape_html_attribute(href));
            html.push_str(
                "\" target=\"_blank\" rel=\"noreferrer noopener\">Open grounded source</a>",
            );
        }
        html.push_str("</div>");
    }
    html.push_str("</div>");
}

fn evidence_grounding_link(
    evidence: &EvidenceReference,
    grounding_lookup: &BTreeMap<String, GroundingLinkTarget>,
) -> Option<(String, &'static str)> {
    let document_id = evidence.document_id.as_deref()?;
    let target = grounding_lookup.get(document_id)?;
    if let Some(quote) = evidence.quote.as_deref() {
        if let Some(region_id) = match_quote_to_region_id(&target.summary, quote) {
            return Some((
                format!("{}#region-{}", target.href, region_id),
                "Open highlighted source",
            ));
        }
    }
    Some((target.href.clone(), "Open grounded source"))
}

fn evidence_anchor_id(evidence: &EvidenceReference) -> String {
    let base = match evidence.kind {
        crate::draft::EvidenceKind::SystemGenerated => format!(
            "evidence-system-generated-{}",
            evidence.origin.as_deref().unwrap_or("system")
        ),
        crate::draft::EvidenceKind::DocumentSpan => format!(
            "evidence-document-span-{}-page-{}",
            evidence
                .document_id
                .as_deref()
                .or(evidence.filename.as_deref())
                .unwrap_or("document"),
            evidence.page.unwrap_or(0)
        ),
        crate::draft::EvidenceKind::Document => format!(
            "evidence-document-{}",
            evidence
                .document_id
                .as_deref()
                .or(evidence.filename.as_deref())
                .unwrap_or("document")
        ),
        crate::draft::EvidenceKind::UserInput => format!(
            "evidence-user-input-{}",
            evidence.origin.as_deref().unwrap_or("user-input")
        ),
    };
    anchor_id("", &base).trim_start_matches('-').to_owned()
}

fn scalar_initial_json(value: &str) -> String {
    if value.is_empty() {
        "null".to_owned()
    } else {
        serde_json::to_string(value).expect("scalar values should serialize")
    }
}

fn bool_initial_json(value: Option<&str>) -> String {
    match value {
        Some("true") => "true".to_owned(),
        Some("false") => "false".to_owned(),
        _ => "null".to_owned(),
    }
}

fn correction_reason_options() -> &'static [(&'static str, &'static str)] {
    &[
        ("ocr_error", "OCR error"),
        (
            "wrong_document_classification",
            "Wrong document classification",
        ),
        (
            "wrong_expense_type_classification",
            "Wrong expense type classification",
        ),
        ("wrong_cross_document_merge", "Wrong cross-document merge"),
        ("missing_required_field", "Missing required field"),
        ("wrong_derived_field", "Wrong derived field"),
        ("stanford_policy_mismatch", "Stanford policy mismatch"),
        (
            "stanford_site_workflow_mismatch",
            "Stanford site workflow mismatch",
        ),
        (
            "unclear_or_undocumented_rule",
            "Unclear or undocumented rule",
        ),
        ("other", "Other"),
    ]
}

fn render_attachments_panel(html: &mut String, packet: &ReviewPacket) {
    html.push_str("<section class=\"panel attachments-panel\">\n");
    html.push_str("<div class=\"panel-heading\"><p class=\"eyebrow\">Attachments</p><h2>Checklist</h2></div>\n");
    if packet.attachment_checklist.is_empty() {
        html.push_str("<p class=\"empty-state\">No projected attachments.</p>\n");
    } else {
        html.push_str("<div class=\"attachment-list\">\n");
        for item in &packet.attachment_checklist {
            html.push_str("<article class=\"attachment-card\">\n<h3>Line ");
            html.push_str(&(item.line_index + 1).to_string());
            html.push_str("</h3>\n<p>");
            html.push_str(&escape_html(
                item.expense_type
                    .as_deref()
                    .unwrap_or("unknown expense type"),
            ));
            html.push_str("</p>\n");
            if let Some(remarks) = item.remarks.as_deref() {
                html.push_str("<p class=\"attachment-remarks\">");
                html.push_str(&escape_html(remarks));
                html.push_str("</p>\n");
            }
            html.push_str("<ul>");
            for filename in &item.filenames {
                html.push_str("<li>");
                html.push_str(&escape_html(filename));
                html.push_str("</li>");
            }
            html.push_str("</ul>\n</article>\n");
        }
        html.push_str("</div>\n");
    }
    html.push_str("</section>\n");
}

fn build_workbench_index(packet: &ReviewPacket) -> WorkbenchIndex {
    let mut index = WorkbenchIndex::default();

    for section in &packet.copy_sections {
        index
            .section_targets
            .insert(section.key.clone(), anchor_id("section", &section.key));
        for instance in &section.instances {
            index
                .instance_targets
                .insert(instance.path.clone(), anchor_id("instance", &instance.path));
            for field in &instance.fields {
                let field_id = anchor_id("field", &field.path);
                index
                    .field_targets
                    .insert(field.path.clone(), field_id.clone());
            }
        }
    }
    index
}

fn issue_target(path: &str, index: &WorkbenchIndex) -> Option<String> {
    if let Some(target) = index.field_targets.get(path) {
        return Some(target.clone());
    }

    if let Some(instance_path) = transaction_line_instance_path(path) {
        if let Some(target) = index.instance_targets.get(instance_path) {
            return Some(target.clone());
        }
    }

    if let Some(section_key) = section_key_for_path(path) {
        if let Some(target) = index.section_targets.get(section_key) {
            return Some(target.clone());
        }
    }

    None
}

fn section_key_for_path(path: &str) -> Option<&str> {
    let remainder = path.strip_prefix("expense_report.")?;
    if remainder.starts_with("transaction_lines[") {
        return Some("transaction_lines");
    }
    match remainder.find('.') {
        Some(dot_index) => Some(&remainder[..dot_index]),
        None => Some(remainder),
    }
}

fn transaction_line_instance_path(path: &str) -> Option<&str> {
    let prefix = "expense_report.transaction_lines[";
    let remainder = path.strip_prefix(prefix)?;
    let closing_index = remainder.find(']')?;
    Some(&path[..prefix.len() + closing_index + 1])
}

fn summary_card(html: &mut String, label: &str, value: &str) {
    html.push_str("<article class=\"summary-card\"><p class=\"summary-label\">");
    html.push_str(&escape_html(label));
    html.push_str("</p><p class=\"summary-value\">");
    html.push_str(&escape_html(value));
    html.push_str("</p></article>\n");
}

fn badge(html: &mut String, value: &str) {
    html.push_str("<span class=\"badge\">");
    html.push_str(&escape_html(value));
    html.push_str("</span>");
}

fn evidence_title(evidence: &EvidenceReference) -> String {
    match evidence.kind {
        crate::draft::EvidenceKind::DocumentSpan => evidence
            .page
            .map(|page| format!("Page {page} excerpt"))
            .unwrap_or_else(|| "Document excerpt".to_owned()),
        crate::draft::EvidenceKind::Document => evidence
            .page
            .map(|page| format!("Page {page}"))
            .unwrap_or_else(|| "Full document".to_owned()),
        crate::draft::EvidenceKind::SystemGenerated => evidence
            .origin
            .as_deref()
            .map(short_origin_label)
            .unwrap_or_else(|| "Derived value".to_owned()),
        crate::draft::EvidenceKind::UserInput => "User-provided value".to_owned(),
    }
}

fn evidence_kind_display(evidence: &EvidenceReference) -> &'static str {
    match evidence.kind {
        crate::draft::EvidenceKind::Document => "document",
        crate::draft::EvidenceKind::DocumentSpan => "excerpt",
        crate::draft::EvidenceKind::SystemGenerated => "system-derived",
        crate::draft::EvidenceKind::UserInput => "user-input",
    }
}

fn evidence_source_label(evidence: &EvidenceReference) -> Option<String> {
    match evidence.kind {
        crate::draft::EvidenceKind::Document | crate::draft::EvidenceKind::DocumentSpan => evidence
            .filename
            .clone()
            .or_else(|| evidence.document_id.clone()),
        crate::draft::EvidenceKind::SystemGenerated => evidence.document_id.clone(),
        crate::draft::EvidenceKind::UserInput => evidence
            .filename
            .clone()
            .or_else(|| evidence.document_id.clone()),
    }
}

fn short_origin_label(origin: &str) -> String {
    humanize_machine_label(origin.rsplit('.').next().unwrap_or(origin))
}

fn humanize_machine_label(value: &str) -> String {
    value
        .split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => {
                    let mut word = String::new();
                    word.push(first.to_ascii_uppercase());
                    word.extend(chars);
                    word
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn anchor_id(prefix: &str, value: &str) -> String {
    let mut id = String::from(prefix);
    id.push('-');
    for ch in value.chars() {
        match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' => id.push(ch.to_ascii_lowercase()),
            _ => id.push('-'),
        }
    }
    while id.contains("--") {
        id = id.replace("--", "-");
    }
    id.trim_matches('-').to_owned()
}

fn filing_status_label(status: FilingStatus) -> &'static str {
    match status {
        FilingStatus::AutomationBlocked => "automation blocked",
        FilingStatus::UserInputRequired => "user input required",
        FilingStatus::ManualReviewRequired => "manual review required",
        FilingStatus::ReadyToFile => "ready to file",
    }
}

fn confidence_level_label(level: crate::ConfidenceLevel) -> &'static str {
    match level {
        crate::ConfidenceLevel::High => "High",
        crate::ConfidenceLevel::Medium => "Medium",
        crate::ConfidenceLevel::Low => "Low",
    }
}

fn confidence_level_class(level: crate::ConfidenceLevel) -> &'static str {
    match level {
        crate::ConfidenceLevel::High => "high",
        crate::ConfidenceLevel::Medium => "medium",
        crate::ConfidenceLevel::Low => "low",
    }
}

fn confidence_level_rank(level: crate::ConfidenceLevel) -> usize {
    match level {
        crate::ConfidenceLevel::High => 0,
        crate::ConfidenceLevel::Medium => 1,
        crate::ConfidenceLevel::Low => 2,
    }
}

fn ocr_status_label(status: crate::OcrComparisonStatus) -> &'static str {
    match status {
        crate::OcrComparisonStatus::Consensus => "two passes agreed",
        crate::OcrComparisonStatus::PartialConsensus => "partial agreement",
        crate::OcrComparisonStatus::Missing => "evidence incomplete",
        crate::OcrComparisonStatus::Divergent => "needs review",
    }
}

fn ocr_status_class(status: crate::OcrComparisonStatus) -> &'static str {
    match status {
        crate::OcrComparisonStatus::Consensus => "status-consensus",
        crate::OcrComparisonStatus::PartialConsensus => "status-partial",
        crate::OcrComparisonStatus::Missing => "status-missing",
        crate::OcrComparisonStatus::Divergent => "status-divergent",
    }
}

fn ocr_status_rank(status: crate::OcrComparisonStatus) -> usize {
    match status {
        crate::OcrComparisonStatus::Consensus => 0,
        crate::OcrComparisonStatus::PartialConsensus => 1,
        crate::OcrComparisonStatus::Missing => 2,
        crate::OcrComparisonStatus::Divergent => 3,
    }
}

fn issue_class_name(class: crate::ReadinessIssueClass) -> &'static str {
    match class {
        crate::ReadinessIssueClass::AutomationGap => "automation-gap",
        crate::ReadinessIssueClass::UserInputRequired => "user-input-required",
        crate::ReadinessIssueClass::ManualReview => "manual-review",
        crate::ReadinessIssueClass::OtherWarning => "other-warning",
    }
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn escape_html_attribute(value: &str) -> String {
    escape_html(value).replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::render_review_workbench_html;
    use crate::bundle_synthesis::{
        synthesize_bundle_projection, synthesize_bundle_projection_with_fx, StaticFxRateProvider,
    };
    use crate::review_packet::build_review_packet;
    use crate::synthetic_documents::{generate_synthetic_packet, SyntheticVariant};

    fn synthetic_documents() -> Vec<crate::ExtractedDocumentFacts> {
        generate_synthetic_packet(SyntheticVariant::Baseline)
            .into_iter()
            .map(|fixture| fixture.expected_facts)
            .collect()
    }

    #[test]
    fn renders_fa_workbench_with_issue_links_and_editable_controls() {
        let provider = StaticFxRateProvider::demo();
        let projection = synthesize_bundle_projection_with_fx(&synthetic_documents(), &provider);
        let packet = build_review_packet(
            &projection.bundle,
            &projection.draft,
            &projection.validation,
        )
        .expect("review packet should build");
        let rendered = render_review_workbench_html(&packet);

        assert!(rendered.contains("<title>Expense Report Review Workbench</title>"));
        assert!(rendered.contains("FA Workbench"));
        assert!(rendered.contains("Jump to field"));
        assert!(rendered.contains("copyFieldValue(this)"));
        assert!(rendered.contains("class=\"field-input field-control\""));
        assert!(rendered.contains("Enter missing value"));
        assert!(rendered.contains("synthetic_flight_itinerary_baseline.md"));
        assert!(rendered.contains("Business meal during travel in Singapore"));
        assert!(rendered.contains("Open source document"));
        assert!(rendered.contains("Save And Recompute"));
        assert!(rendered.contains("review-session/save"));
        assert!(rendered.contains("target=\"_blank\" rel=\"noreferrer noopener\""));
        assert!(!rendered.contains("document-preview-backdrop"));
        assert!(!rendered.contains("document-preview-drawer"));
        assert!(!rendered.contains("data-document-preview-close"));
    }

    #[test]
    fn workbench_filters_irrelevant_optional_fields_from_line_cards() {
        let provider = StaticFxRateProvider::demo();
        let projection = synthesize_bundle_projection_with_fx(&synthetic_documents(), &provider);
        let packet = build_review_packet(
            &projection.bundle,
            &projection.draft,
            &projection.validation,
        )
        .expect("review packet should build");
        let rendered = render_review_workbench_html(&packet);

        assert!(!rendered.contains("Shared With Transaction Number"));
        assert!(!rendered.contains("Per Subject Amount"));
        assert!(rendered.contains("Hotel Name"));
        assert!(rendered.contains("Venue Name"));
    }

    #[test]
    fn no_fx_workbench_still_shows_automation_blocked_status() {
        let projection = synthesize_bundle_projection(&synthetic_documents());
        let packet = build_review_packet(
            &projection.bundle,
            &projection.draft,
            &projection.validation,
        )
        .expect("review packet should build");
        let rendered = render_review_workbench_html(&packet);

        assert!(rendered.contains("automation blocked"));
        assert!(rendered.contains("expense_report.transaction_summary.total_usd"));
    }

    #[test]
    fn issue_fallback_links_cover_section_level_anchors() {
        let provider = StaticFxRateProvider::demo();
        let projection = synthesize_bundle_projection_with_fx(&synthetic_documents(), &provider);
        let packet = build_review_packet(
            &projection.bundle,
            &projection.draft,
            &projection.validation,
        )
        .expect("review packet should build");
        let rendered = render_review_workbench_html(&packet);

        assert!(rendered.contains("jumpToField(event"));
        assert!(
            rendered.contains("href=\"#field-expense-report-general-information-authorized-by\"")
        );
    }

    #[test]
    fn workbench_renders_inline_evidence_in_field_cards() {
        let provider = StaticFxRateProvider::demo();
        let projection = synthesize_bundle_projection_with_fx(&synthetic_documents(), &provider);
        let packet = build_review_packet(
            &projection.bundle,
            &projection.draft,
            &projection.validation,
        )
        .expect("review packet should build");
        let rendered = render_review_workbench_html(&packet);

        assert!(rendered.contains("class=\"field-evidence\""));
        assert!(rendered.contains("Evidence ("));
        assert!(rendered.contains("Page 1 excerpt"));
        assert!(rendered.contains("Open source document"));
        assert!(rendered.contains("target=\"_blank\" rel=\"noreferrer noopener\""));
        assert!(!rendered.contains("data-document-quote="));
    }

    #[test]
    fn workbench_renders_structured_list_editors_for_nested_repeaters() {
        let provider = StaticFxRateProvider::demo();
        let projection = synthesize_bundle_projection_with_fx(&synthetic_documents(), &provider);
        let packet = build_review_packet(
            &projection.bundle,
            &projection.draft,
            &projection.validation,
        )
        .expect("review packet should build");
        let rendered = render_review_workbench_html(&packet);

        assert!(rendered.contains("structured-list-editor"));
        assert!(rendered.contains("Add row"));
        assert!(rendered.contains("Attendees"));
        assert!(rendered.contains("Source Documents"));
    }

    #[test]
    fn workbench_css_contains_issues_panel_overflow_constraint() {
        let provider = StaticFxRateProvider::demo();
        let projection = synthesize_bundle_projection_with_fx(&synthetic_documents(), &provider);
        let packet = build_review_packet(
            &projection.bundle,
            &projection.draft,
            &projection.validation,
        )
        .expect("review packet should build");
        let rendered = render_review_workbench_html(&packet);

        assert!(
            rendered.contains("max-height:") && rendered.contains("overflow-y: auto"),
            "issues panel must have max-height and overflow-y to prevent viewport overflow"
        );
    }

    #[test]
    fn workbench_css_contains_document_snapshot_signal_styles() {
        let provider = StaticFxRateProvider::demo();
        let projection = synthesize_bundle_projection_with_fx(&synthetic_documents(), &provider);
        let packet = build_review_packet(
            &projection.bundle,
            &projection.draft,
            &projection.validation,
        )
        .expect("review packet should build");
        let rendered = render_review_workbench_html(&packet);

        assert!(rendered.contains(".document-snapshot-signal.high"));
        assert!(rendered.contains(".document-snapshot-signal.medium"));
        assert!(rendered.contains(".document-snapshot-signal.low"));
        assert!(rendered.contains(".document-snapshot-signal.grounded"));
        assert!(rendered.contains(".document-snapshot-signal.status.status-consensus"));
        assert!(rendered.contains(".field-ocr-signal"));
        assert!(rendered.contains(".ocr-field-status.status-divergent"));
    }

    #[test]
    fn workbench_readonly_fields_show_review_computed_label() {
        let provider = StaticFxRateProvider::demo();
        let projection = synthesize_bundle_projection_with_fx(&synthetic_documents(), &provider);
        let packet = build_review_packet(
            &projection.bundle,
            &projection.draft,
            &projection.validation,
        )
        .expect("review packet should build");
        let rendered = render_review_workbench_html(&packet);

        assert!(
            rendered.contains("Review computed value"),
            "readonly computed fields should show 'Review computed value' label"
        );
    }

    #[test]
    fn workbench_editable_present_fields_show_review_or_edit_label() {
        let provider = StaticFxRateProvider::demo();
        let projection = synthesize_bundle_projection_with_fx(&synthetic_documents(), &provider);
        let packet = build_review_packet(
            &projection.bundle,
            &projection.draft,
            &projection.validation,
        )
        .expect("review packet should build");
        let rendered = render_review_workbench_html(&packet);

        assert!(
            rendered.contains("Review or edit value"),
            "editable present fields should still show 'Review or edit value'"
        );
    }

    #[test]
    fn workbench_missing_fields_show_enter_missing_label() {
        let provider = StaticFxRateProvider::demo();
        let projection = synthesize_bundle_projection_with_fx(&synthetic_documents(), &provider);
        let packet = build_review_packet(
            &projection.bundle,
            &projection.draft,
            &projection.validation,
        )
        .expect("review packet should build");
        let rendered = render_review_workbench_html(&packet);

        assert!(
            rendered.contains("Enter missing value"),
            "missing editable fields should show 'Enter missing value'"
        );
    }

    #[test]
    fn workbench_includes_beforeunload_unsaved_changes_guard() {
        let provider = StaticFxRateProvider::demo();
        let projection = synthesize_bundle_projection_with_fx(&synthetic_documents(), &provider);
        let packet = build_review_packet(
            &projection.bundle,
            &projection.draft,
            &projection.validation,
        )
        .expect("review packet should build");
        let rendered = render_review_workbench_html(&packet);

        assert!(
            rendered.contains("beforeunload"),
            "workbench must include a beforeunload guard to prevent accidental data loss"
        );
        assert!(
            rendered.contains("hasDirtyFields"),
            "beforeunload guard should check hasDirtyFields before firing"
        );
        assert!(
            rendered.contains("event.returnValue=''"),
            "beforeunload guard must set returnValue for cross-browser compatibility"
        );
    }

    #[test]
    fn workbench_relaxes_issues_panel_scroll_on_narrow_layouts() {
        let provider = StaticFxRateProvider::demo();
        let projection = synthesize_bundle_projection_with_fx(&synthetic_documents(), &provider);
        let packet = build_review_packet(
            &projection.bundle,
            &projection.draft,
            &projection.validation,
        )
        .expect("review packet should build");
        let rendered = render_review_workbench_html(&packet);

        assert!(
            rendered.contains("max-height: none"),
            "narrow layout must remove the issues panel max-height so it expands naturally"
        );
        assert!(
            rendered.contains("overflow-y: visible"),
            "narrow layout must reset overflow-y so the panel is not internally scrollable"
        );
    }

    #[test]
    fn workbench_includes_ctrl_s_save_shortcut() {
        let provider = StaticFxRateProvider::demo();
        let projection = synthesize_bundle_projection_with_fx(&synthetic_documents(), &provider);
        let packet = build_review_packet(
            &projection.bundle,
            &projection.draft,
            &projection.validation,
        )
        .expect("review packet should build");
        let rendered = render_review_workbench_html(&packet);

        assert!(
            rendered.contains("event.metaKey||event.ctrlKey")
                && rendered.contains("event.key==='s'"),
            "workbench must include Ctrl/Cmd+S keyboard shortcut for saving"
        );
    }

    #[test]
    fn workbench_renders_ocr_diff_links_for_document_snapshots() {
        let provider = StaticFxRateProvider::demo();
        let projection = synthesize_bundle_projection_with_fx(&synthetic_documents(), &provider);
        let packet = crate::build_review_packet_with_ocr_comparisons(
            &projection.bundle,
            &projection.draft,
            &crate::summarize_validation_readiness(&projection.validation),
            &[crate::DocumentOcrComparisonSummary {
                document_id: "synthetic_receipt_baseline".to_owned(),
                compared_pass_count: 2,
                overall_confidence: crate::ConfidenceLevel::Medium,
                disagreement_count: 1,
                divergent_fields: vec!["total_paid".to_owned()],
                consistency_warning_count: 1,
                consistency_notes: vec!["Line items did not sum to total.".to_owned()],
                field_summaries: vec![],
            }],
        )
        .expect("review packet should build");
        let rendered = render_review_workbench_html(&packet);

        assert!(rendered.contains("OCR cross-check"));
        assert!(rendered.contains("Open OCR inspection"));
        assert!(
            rendered.contains("artifact/ocr_inspection/synthetic_receipt_baseline/inspection.html")
        );
        assert!(rendered.contains("Open OCR diff"));
        assert!(rendered
            .contains("artifact/ocr_pass_comparisons/synthetic_receipt_baseline/comparison.html"));
        assert!(rendered.contains("Consistency warnings: 1"));
        assert!(rendered.contains("Line items did not sum to total."));
    }

    #[test]
    fn workbench_renders_document_snapshot_confidence_and_grounding_badges() {
        let packet = crate::review_packet::ReviewPacket {
            summary: crate::review_packet::PacketSummary {
                filing_status: crate::review_packet::FilingStatus::ManualReviewRequired,
                payee_name: Some("Olivia Park".to_owned()),
                event_name: Some("Receipt OCR review".to_owned()),
                trip_window: None,
                report_total_usd: None,
                category: None,
                transaction_type: None,
                transaction_line_count: 0,
                document_count: 1,
                readiness: crate::review_packet::ReviewReadinessSummary {
                    automation_gap_count: 0,
                    user_input_gap_count: 0,
                    manual_review_count: 1,
                    other_warning_count: 0,
                },
                confidence: crate::review_packet::ConfidenceSummary {
                    high: 0,
                    medium: 1,
                    low: 0,
                    needs_review: 1,
                },
            },
            issues_queue: Vec::new(),
            copy_sections: Vec::new(),
            attachment_checklist: Vec::new(),
            document_snapshots: vec![crate::review_packet::DocumentSnapshotCard {
                document_id: "doc_receipt".to_owned(),
                filename: "receipt.png".to_owned(),
                kind: "receipt".to_owned(),
                extraction_status: "complete".to_owned(),
                used_in_bundle: true,
                projected_to_filing: false,
                status_label: "parsed for bundle context only".to_owned(),
                summary_fields: vec![
                    crate::review_packet::DocumentSnapshotField {
                        label: "Merchant".to_owned(),
                        value: "BOOK TALK".to_owned(),
                        ocr_confidence: Some(crate::ConfidenceLevel::High),
                        ocr_status: Some(crate::OcrComparisonStatus::Consensus),
                        grounded: true,
                    },
                    crate::review_packet::DocumentSnapshotField {
                        label: "Total".to_owned(),
                        value: "MYR 80.90".to_owned(),
                        ocr_confidence: Some(crate::ConfidenceLevel::Low),
                        ocr_status: Some(crate::OcrComparisonStatus::Divergent),
                        grounded: false,
                    },
                ],
                issue_messages: Vec::new(),
                ocr_comparison: None,
                ocr_comparison_href: None,
                ocr_grounding: None,
                ocr_grounding_href: None,
            }],
        };

        let rendered = render_review_workbench_html(&packet);

        assert!(rendered.contains("ocr High"));
        assert!(rendered.contains("ocr Low"));
        assert!(rendered.contains("grounded"));
    }

    #[test]
    fn workbench_renders_highlighted_grounded_source_links() {
        let packet = crate::review_packet::ReviewPacket {
            summary: crate::review_packet::PacketSummary {
                filing_status: crate::review_packet::FilingStatus::ManualReviewRequired,
                payee_name: Some("Olivia Park".to_owned()),
                event_name: Some("Receipt OCR review".to_owned()),
                trip_window: None,
                report_total_usd: None,
                category: None,
                transaction_type: None,
                transaction_line_count: 0,
                document_count: 1,
                readiness: crate::review_packet::ReviewReadinessSummary {
                    automation_gap_count: 0,
                    user_input_gap_count: 0,
                    manual_review_count: 1,
                    other_warning_count: 0,
                },
                confidence: crate::review_packet::ConfidenceSummary {
                    high: 0,
                    medium: 1,
                    low: 0,
                    needs_review: 1,
                },
            },
            issues_queue: Vec::new(),
            copy_sections: vec![crate::review_packet::CopySection {
                key: "general_information".to_owned(),
                label: "General Information".to_owned(),
                repeated: false,
                instances: vec![crate::review_packet::CopySectionInstance {
                    path: "expense_report.general_information".to_owned(),
                    label: "General Information".to_owned(),
                    fields: vec![crate::review_packet::CopyField {
                        path: "expense_report.general_information.event_name".to_owned(),
                        label: "Event Name".to_owned(),
                        control: crate::FieldControl::Text,
                        allowed_values: Vec::new(),
                        collection_columns: Vec::new(),
                        collection_rows: Vec::new(),
                        value: Some("Receipt OCR review".to_owned()),
                        present: true,
                        needs_review: true,
                        required: true,
                        source: Some("T3".to_owned()),
                        entry_mode: crate::FieldEntryMode::ModelPrefillReview,
                        evidence: vec![crate::EvidenceReference {
                            kind: crate::EvidenceKind::DocumentSpan,
                            document_id: Some("doc_receipt".to_owned()),
                            filename: Some("receipt.png".to_owned()),
                            page: Some(1),
                            quote: Some("MYR 80.90".to_owned()),
                            origin: None,
                        }],
                    }],
                }],
            }],
            attachment_checklist: Vec::new(),
            document_snapshots: vec![crate::review_packet::DocumentSnapshotCard {
                document_id: "doc_receipt".to_owned(),
                filename: "receipt.png".to_owned(),
                kind: "receipt".to_owned(),
                extraction_status: "complete".to_owned(),
                used_in_bundle: true,
                projected_to_filing: false,
                status_label: "parsed for bundle context only".to_owned(),
                summary_fields: Vec::new(),
                issue_messages: Vec::new(),
                ocr_comparison: None,
                ocr_comparison_href: None,
                ocr_grounding: Some(crate::DocumentOcrGroundingSummary {
                    document_id: "doc_receipt".to_owned(),
                    geometry_source: crate::OcrGeometrySource::Gemini,
                    geometry_available: true,
                    grounding_preprocess_variant: Some(crate::OcrPreprocessVariant::Original),
                    preview_href: Some(
                        "artifact/ocr_grounding/doc_receipt/grounded_preview.html".to_owned(),
                    ),
                    regions: vec![crate::GroundedRegionSummary {
                        region_id: "total_paid".to_owned(),
                        page_number: 1,
                        kind: crate::OcrRegionKind::ValueCandidate,
                        text: "MYR 80.90".to_owned(),
                    }],
                }),
                ocr_grounding_href: Some(
                    "artifact/ocr_grounding/doc_receipt/grounded_preview.html".to_owned(),
                ),
            }],
        };
        let rendered = render_review_workbench_html(&packet);

        assert!(rendered.contains("Open highlighted source"));
        assert!(rendered.contains(
            "artifact/ocr_grounding/doc_receipt/grounded_preview.html#region-total_paid"
        ));
        assert!(rendered.contains("Open grounded source"));
    }

    #[test]
    fn workbench_renders_field_level_ocr_signal_panel() {
        let packet = crate::review_packet::ReviewPacket {
            summary: crate::review_packet::PacketSummary {
                filing_status: crate::review_packet::FilingStatus::ManualReviewRequired,
                payee_name: Some("Olivia Park".to_owned()),
                event_name: Some("Receipt OCR review".to_owned()),
                trip_window: None,
                report_total_usd: None,
                category: None,
                transaction_type: None,
                transaction_line_count: 0,
                document_count: 1,
                readiness: crate::review_packet::ReviewReadinessSummary {
                    automation_gap_count: 0,
                    user_input_gap_count: 0,
                    manual_review_count: 1,
                    other_warning_count: 0,
                },
                confidence: crate::review_packet::ConfidenceSummary {
                    high: 0,
                    medium: 1,
                    low: 0,
                    needs_review: 1,
                },
            },
            issues_queue: Vec::new(),
            copy_sections: vec![crate::review_packet::CopySection {
                key: "general_information".to_owned(),
                label: "General Information".to_owned(),
                repeated: false,
                instances: vec![crate::review_packet::CopySectionInstance {
                    path: "expense_report.general_information".to_owned(),
                    label: "General Information".to_owned(),
                    fields: vec![crate::review_packet::CopyField {
                        path: "expense_report.general_information.event_name".to_owned(),
                        label: "Event Name".to_owned(),
                        control: crate::FieldControl::Text,
                        allowed_values: Vec::new(),
                        collection_columns: Vec::new(),
                        collection_rows: Vec::new(),
                        value: Some("MYR 80.90".to_owned()),
                        present: true,
                        needs_review: true,
                        required: true,
                        source: Some("T3".to_owned()),
                        entry_mode: crate::FieldEntryMode::ModelPrefillReview,
                        evidence: vec![crate::EvidenceReference {
                            kind: crate::EvidenceKind::DocumentSpan,
                            document_id: Some("doc_receipt".to_owned()),
                            filename: Some("receipt.png".to_owned()),
                            page: Some(1),
                            quote: Some("MYR 80.90".to_owned()),
                            origin: None,
                        }],
                    }],
                }],
            }],
            attachment_checklist: Vec::new(),
            document_snapshots: vec![crate::review_packet::DocumentSnapshotCard {
                document_id: "doc_receipt".to_owned(),
                filename: "receipt.png".to_owned(),
                kind: "receipt".to_owned(),
                extraction_status: "complete".to_owned(),
                used_in_bundle: true,
                projected_to_filing: false,
                status_label: "parsed for bundle context only".to_owned(),
                summary_fields: Vec::new(),
                issue_messages: Vec::new(),
                ocr_comparison: Some(crate::DocumentOcrComparisonSummary {
                    document_id: "doc_receipt".to_owned(),
                    compared_pass_count: 2,
                    overall_confidence: crate::ConfidenceLevel::Low,
                    disagreement_count: 1,
                    divergent_fields: vec!["total_paid".to_owned()],
                    consistency_warning_count: 1,
                    consistency_notes: vec!["Total did not match subtotal + tax.".to_owned()],
                    field_summaries: vec![crate::OcrFieldComparisonSummary {
                        field: "total_paid".to_owned(),
                        status: crate::OcrComparisonStatus::Divergent,
                        confidence: crate::ConfidenceLevel::Low,
                        consensus_value: Some("MYR 80.90".to_owned()),
                    }],
                }),
                ocr_comparison_href: Some(
                    "artifact/ocr_pass_comparisons/doc_receipt/comparison.html".to_owned(),
                ),
                ocr_grounding: Some(crate::DocumentOcrGroundingSummary {
                    document_id: "doc_receipt".to_owned(),
                    geometry_source: crate::OcrGeometrySource::Gemini,
                    geometry_available: true,
                    grounding_preprocess_variant: Some(
                        crate::OcrPreprocessVariant::ContrastBoosted,
                    ),
                    preview_href: Some(
                        "artifact/ocr_grounding/doc_receipt/grounded_preview.html".to_owned(),
                    ),
                    regions: vec![crate::GroundedRegionSummary {
                        region_id: "total_paid".to_owned(),
                        page_number: 1,
                        kind: crate::OcrRegionKind::ValueCandidate,
                        text: "MYR 80.90".to_owned(),
                    }],
                }),
                ocr_grounding_href: Some(
                    "artifact/ocr_grounding/doc_receipt/grounded_preview.html".to_owned(),
                ),
            }],
        };

        let rendered = render_review_workbench_html(&packet);

        assert!(rendered.contains("needs review"));
        assert!(rendered.contains("ocr Low"));
        assert!(rendered
            .contains("The linked receipt also failed an internal amount-consistency check."));
        assert!(rendered.contains("Fields To Double-Check"));
        assert!(rendered.contains("Jump to field"));
        assert!(rendered.contains("Open OCR diff"));
        assert!(rendered.contains("Open grounded source"));
        assert!(rendered.contains("Recovered via contrast_boosted preprocessing"));
    }
}
