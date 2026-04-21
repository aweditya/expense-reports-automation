use std::collections::BTreeMap;

use crate::draft::EvidenceReference;
use crate::field_conventions::{FieldControl, FieldEntryMode};
use crate::review_packet::{CopyCollectionColumn, CopyField, CopySection, FilingStatus, ReviewPacket};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct WorkbenchIndex {
    field_targets: BTreeMap<String, String>,
    instance_targets: BTreeMap<String, String>,
    section_targets: BTreeMap<String, String>,
}

pub fn render_fa_workbench_html(packet: &ReviewPacket) -> String {
    let index = build_workbench_index(packet);
    let mut html = String::new();
    html.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    html.push_str(
        "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>FA Workbench</title>\n",
    );
    html.push_str("<style>\n");
    html.push_str(
        "body{margin:0;background:#f6f1e8;color:#1f1a17;font-family:Georgia,'Times New Roman',serif;}\
         .shell{max-width:1340px;margin:0 auto;padding:24px 24px 40px;}\
         .hero,.toolbar,.issues-panel,.editor-panel,.source-panel,.field-card,.section-card,.instance-card,.source-card{background:#fff;border:1px solid #e3dac9;border-radius:20px;box-shadow:0 12px 28px rgba(41,27,16,.05);}\
         .hero{padding:24px;margin-bottom:18px;}\
         .eyebrow{margin:0 0 8px;color:#9a5f33;font-size:12px;font-weight:700;letter-spacing:.18em;text-transform:uppercase;}\
         h1,h2,h3,h4,p{margin:0;}\
         h1{font-size:40px;line-height:1.05;}\
         h2{font-size:28px;line-height:1.12;}\
         h3{font-size:22px;line-height:1.18;}\
         h4{font-size:18px;line-height:1.2;}\
         .hero-status{margin-top:12px;font-size:18px;font-weight:700;color:#2c5845;text-transform:capitalize;}\
         .hero-subtitle,.hero-copy,.field-source,.field-guidance,.issue-message,.toolbar-status,.toolbar-count,.source-status,.source-meta,.evidence-copy,.evidence-detail,.field-hint{color:#655a50;}\
         .hero-subtitle{margin-top:8px;font-size:18px;}\
         .hero-grid{display:grid;grid-template-columns:minmax(0,1.6fr) minmax(0,1fr);gap:16px;align-items:start;}\
         .summary-grid{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:12px;}\
         .summary-card{padding:16px;border:1px solid #eadfcd;border-radius:16px;background:#fbf7f0;}\
         .summary-label{font-size:12px;font-weight:700;letter-spacing:.12em;text-transform:uppercase;color:#816f5f;}\
         .summary-value{margin-top:10px;font-size:24px;line-height:1.2;}\
         .toolbar{padding:18px 20px;margin-bottom:18px;}\
         .toolbar-main,.field-head,.issue-head,.source-head{display:flex;justify-content:space-between;align-items:flex-start;gap:14px;}\
         .toolbar-meta{margin-top:6px;color:#655a50;}\
         .toolbar-actions,.toolbar-links,.field-badges,.source-links,.evidence-actions{display:flex;flex-wrap:wrap;gap:10px;}\
         .toolbar-subrow{display:flex;justify-content:space-between;align-items:center;gap:12px;margin-top:12px;padding-top:12px;border-top:1px solid #ece2d4;}\
         .primary-button,.secondary-button,.ghost-button,.issue-link,.nav-link,.doc-link{appearance:none;border:0;border-radius:999px;padding:10px 15px;font-size:14px;font-weight:700;text-decoration:none;cursor:pointer;}\
         .primary-button,.issue-link{background:#2f5b53;color:#fff;}\
         .secondary-button,.nav-link{background:#efe6da;color:#362d26;}\
         .ghost-button{background:#1f1a17;color:#fff;}\
         .layout{display:grid;grid-template-columns:330px minmax(0,1fr);gap:18px;align-items:start;}\
         .issues-panel,.editor-panel,.source-panel{padding:18px 20px;}\
         .issues-panel{position:sticky;top:18px;max-height:calc(100vh - 36px);overflow-y:auto;}\
         .issue-list,.field-list,.section-stack,.instance-stack,.source-grid,.evidence-list{display:grid;gap:14px;}\
         .issue-card{padding:15px;border:1px solid #eadfcd;border-radius:16px;background:#fbf7f0;}\
         .issue-class{display:inline-flex;align-items:center;border-radius:999px;padding:5px 10px;font-size:12px;font-weight:700;letter-spacing:.08em;text-transform:uppercase;}\
         .issue-class.missing{background:#f4ddd9;color:#7c3128;}\
         .issue-class.review{background:#f6e6ce;color:#7f511f;}\
         .issue-class.ready{background:#dcebd8;color:#21472f;}\
         .issue-label{font-size:18px;font-weight:700;margin-top:8px;}\
         .issue-message{margin-top:8px;line-height:1.45;}\
         .editor-panel{min-width:0;}\
         .editor-header{display:flex;justify-content:space-between;align-items:flex-start;gap:12px;margin-bottom:14px;}\
         .section-card{padding:18px;background:#fbf8f2;}\
         .instance-card{padding:16px;border:1px solid #e7dece;border-radius:18px;background:#fff;}\
         .field-list{grid-template-columns:repeat(2,minmax(0,1fr));}\
         .field-card{padding:16px;border:1px solid #eadfcd;border-radius:16px;box-shadow:none;}\
         .field-card.missing{border-style:dashed;}\
         .field-card.readonly{background:#faf7f1;}\
         .field-label{font-size:19px;font-weight:700;}\
         .badge{display:inline-flex;align-items:center;border-radius:999px;padding:5px 9px;font-size:12px;font-weight:700;letter-spacing:.06em;text-transform:uppercase;background:#efe6da;color:#54463a;}\
         .badge.status-ready{background:#dcebd8;color:#21472f;}\
         .badge.status-missing{background:#f4ddd9;color:#7c3128;}\
         .badge.status-review{background:#f6e6ce;color:#7f511f;}\
         .badge.status-computed{background:#ece4da;color:#5c493a;}\
         .field-editor{margin-top:12px;}\
         .field-editor-label{display:block;font-size:12px;font-weight:700;letter-spacing:.12em;text-transform:uppercase;color:#7d6e61;margin-bottom:8px;}\
         .field-input, .structured-row-input, .field-reason-select, .field-note-input{width:100%;box-sizing:border-box;border:1px solid #d7c9b7;border-radius:12px;background:#fff;color:#1f1a17;font:inherit;padding:10px 12px;}\
         textarea.field-input,.field-note-input{min-height:108px;resize:vertical;}\
         .field-input[readonly], .field-input[disabled], .structured-row-input[disabled]{background:#f2ece3;color:#6d6258;}\
         .field-guidance{margin-top:10px;font-size:14px;line-height:1.45;}\
         .field-actions{display:flex;justify-content:flex-end;margin-top:10px;}\
         .field-dirty-badge{display:inline-flex;}\
         .field-dirty-badge[hidden]{display:none;}\
         details.field-evidence,details.field-review-controls{margin-top:12px;border-top:1px solid #ece2d4;padding-top:12px;}\
         details summary{cursor:pointer;font-weight:700;color:#2b4f69;}\
         .evidence-list{margin-top:10px;}\
         .evidence-card{padding:12px;border:1px solid #e8ddcf;border-radius:14px;background:#fbf7f1;}\
         .evidence-copy{margin-top:8px;line-height:1.45;}\
         .evidence-detail{margin-top:8px;font-size:14px;}\
         .structured-list-editor{display:grid;gap:10px;}\
         .structured-list-copy{color:#655a50;font-size:14px;line-height:1.45;}\
         .structured-row{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:10px;padding:12px;border:1px solid #e8ddcf;border-radius:14px;background:#fbf7f1;}\
         .structured-cell{display:grid;gap:6px;}\
         .structured-cell-label{font-size:12px;font-weight:700;letter-spacing:.08em;text-transform:uppercase;color:#7d6e61;}\
         .structured-row-remove{justify-self:start;appearance:none;border:0;border-radius:999px;padding:8px 12px;background:#efe6da;color:#362d26;font-size:13px;font-weight:700;cursor:pointer;}\
         .source-panel{margin-top:18px;}\
         .source-grid{grid-template-columns:repeat(2,minmax(0,1fr));margin-top:14px;}\
         .source-card{padding:15px;border:1px solid #eadfcd;border-radius:16px;background:#fbf7f0;box-shadow:none;}\
         .source-status{margin-top:8px;font-size:14px;line-height:1.4;}\
         .source-meta{margin-top:10px;font-size:14px;}\
         @media (max-width: 1200px){.layout{grid-template-columns:1fr;}.issues-panel{position:static;max-height:none;overflow-y:visible;}.hero-grid,.field-list,.source-grid{grid-template-columns:1fr;}}\
         @media (max-width: 760px){.shell{padding:18px 14px 28px;}.toolbar-main,.toolbar-subrow,.editor-header,.field-head,.issue-head,.source-head{display:grid;}.summary-grid{grid-template-columns:1fr;}.structured-row{grid-template-columns:1fr;}}",
    );
    html.push_str("\n</style>\n");
    html.push_str("<script>\n");
    html.push_str(
        "const reviewSessionState={currentDraftVersionId:null,saveInFlight:false};\
         function fieldControl(card){return card?card.querySelector('.field-control'):null;}\
         function issueCountLabel(count){return count===1?'1 change pending':`${count} changes pending`;}\
         function jumpToField(event,targetId){const target=document.getElementById(targetId);if(!target){return;}event.preventDefault();target.scrollIntoView({behavior:'smooth',block:'center'});window.location.hash=targetId;const focusTarget=target.querySelector('.field-control, .structured-row-input');if(focusTarget){focusTarget.focus();if(typeof focusTarget.select==='function'){focusTarget.select();}}}\
         function structuredRowsPayload(editor){if(!editor){return [];}const rows=[];for(const row of editor.querySelectorAll('.structured-row')){const values={};let hasAnyValue=false;for(const input of row.querySelectorAll('.structured-row-input')){const key=input.dataset.columnKey||'';let value=('value' in input)?input.value:'';if(input.dataset.columnControl==='checkbox'){value=input.value;}if(value!==''&&value!==null){hasAnyValue=true;}values[key]=value;}if(hasAnyValue){rows.push(values);}}return rows;}\
         function parseJsonData(value,fallback){if(!value){return fallback;}try{return JSON.parse(value);}catch(_err){return fallback;}}\
         function normalizeFieldValue(control){if(!control){return null;}if(control.classList.contains('structured-list-editor')){const columns=parseJsonData(control.dataset.columnsJson,'[]');const rows=structuredRowsPayload(control).map((row)=>{const obj={};for(const column of columns){const raw=row[column.key]??'';if(raw===''){continue;}if(column.control==='checkbox'){obj[column.key]=raw==='true';}else{obj[column.key]=raw;}}return obj;});return rows.length===0?[]:rows;}if(control.dataset.control==='checkbox'){if(control.value===''){return null;}return control.value==='true';}if('value' in control){return control.value===''?null:control.value;}return null;}\
         function initialFieldValue(control){if(!control){return null;}const fallback=control.classList.contains('structured-list-editor')?[]:null;return parseJsonData(control.dataset.initialJson,fallback);}\
         function valuesEqual(left,right){return JSON.stringify(left)===JSON.stringify(right);}\
         function fieldReasonSelect(card){return card.querySelector('.field-reason-select');}\
         function fieldNoteInput(card){return card.querySelector('.field-note-input');}\
         function fieldConfirmInput(card){return card.querySelector('.field-confirm-input');}\
         function updateFieldDirtyState(card){const control=fieldControl(card);if(!control){return;}const edited=!valuesEqual(initialFieldValue(control),normalizeFieldValue(control));const confirmed=Boolean(fieldConfirmInput(card)?.checked);card.classList.toggle('dirty',edited||confirmed);const badge=card.querySelector('.field-dirty-badge');if(badge){badge.hidden=!(edited||confirmed);}}\
         function refreshDirtySummary(){const dirtyCards=[...document.querySelectorAll('.field-card.dirty')];const counter=document.getElementById('pending-change-count');if(counter){counter.textContent=issueCountLabel(dirtyCards.length);}const saveButton=document.getElementById('save-review-button');const resetButton=document.getElementById('reset-review-button');if(saveButton){saveButton.disabled=reviewSessionState.saveInFlight||dirtyCards.length===0;}if(resetButton){resetButton.disabled=reviewSessionState.saveInFlight||dirtyCards.length===0;}}\
         function syncCardStateFromEventTarget(target){const card=target.closest('.field-card');if(!card){return;}updateFieldDirtyState(card);refreshDirtySummary();}\
         function structuredRowTemplate(editor,rowValues){const columns=parseJsonData(editor.dataset.columnsJson,[]);const row=document.createElement('div');row.className='structured-row';for(const column of columns){const cell=document.createElement('label');cell.className='structured-cell';const label=document.createElement('span');label.className='structured-cell-label';label.textContent=column.label;cell.appendChild(label);let input;if(column.control==='select'){input=document.createElement('select');const blank=document.createElement('option');blank.value='';blank.textContent='';input.appendChild(blank);for(const optionValue of column.allowed_values||[]){const option=document.createElement('option');option.value=optionValue;option.textContent=optionValue.replaceAll('_',' ');input.appendChild(option);}}else if(column.control==='checkbox'){input=document.createElement('select');[['','Unset'],['true','Yes'],['false','No']].forEach(([value,labelText])=>{const option=document.createElement('option');option.value=value;option.textContent=labelText;input.appendChild(option);});}else if(column.control==='date'){input=document.createElement('input');input.type='date';}else{input=document.createElement(column.control==='textarea'?'textarea':'input');if(input.tagName==='INPUT'){input.type='text';if(column.control==='currency'||column.control==='number'){input.inputMode='decimal';}}}input.className='structured-row-input';input.dataset.columnKey=column.key;input.dataset.columnControl=column.control;input.value=(rowValues&&rowValues[column.key])||'';input.addEventListener('input',()=>syncCardStateFromEventTarget(input));input.addEventListener('change',()=>syncCardStateFromEventTarget(input));cell.appendChild(input);row.appendChild(cell);}const removeButton=document.createElement('button');removeButton.type='button';removeButton.className='structured-row-remove';removeButton.textContent='Remove row';removeButton.addEventListener('click',()=>{row.remove();syncCardStateFromEventTarget(editor);});row.appendChild(removeButton);return row;}\
         function addStructuredListRow(button){const editor=button.closest('.structured-list-editor');if(!editor){return;}const rows=editor.querySelector('.structured-list-rows');if(!rows){return;}rows.appendChild(structuredRowTemplate(editor,{}));syncCardStateFromEventTarget(editor);}\
         function resetReviewForm(){for(const card of document.querySelectorAll('.field-card')){const control=fieldControl(card);if(!control){continue;}const initial=initialFieldValue(control);if(control.classList.contains('structured-list-editor')){const rows=control.querySelector('.structured-list-rows');if(rows){rows.innerHTML='';for(const rowValues of Array.isArray(initial)?initial:[]){rows.appendChild(structuredRowTemplate(control,rowValues));}}}else if(control.dataset.control==='checkbox'){control.value=initial===null?'':String(initial);}else if('value' in control){control.value=initial??'';}const reason=fieldReasonSelect(card);const note=fieldNoteInput(card);const confirm=fieldConfirmInput(card);if(reason){reason.value='';}if(note){note.value='';}if(confirm){confirm.checked=false;}updateFieldDirtyState(card);}setWorkbenchStatus('Unsaved review edits cleared.','neutral');refreshDirtySummary();}\
         async function loadReviewSessionSummary(){try{const response=await fetch('review-session',{headers:{'Accept':'application/json'}});if(!response.ok){throw new Error(`session lookup failed (${response.status})`);}const payload=await response.json();reviewSessionState.currentDraftVersionId=payload.current_draft_version_id;const versionLabel=document.getElementById('current-draft-version');if(versionLabel){versionLabel.textContent=`v${payload.current_draft_version_id}`;}const filingStatus=document.getElementById('current-filing-status');if(filingStatus){filingStatus.textContent=payload.filing_status.replaceAll('_',' ');}}catch(err){setWorkbenchStatus(`Unable to load review session metadata: ${err.message}`,'error');}}\
         function buildRevisionPayload(){const fieldEdits=[];const confirmedReviewPaths=[];const annotations=[];for(const card of document.querySelectorAll('.field-card')){const control=fieldControl(card);if(!control||card.classList.contains('readonly')){if(fieldConfirmInput(card)?.checked){confirmedReviewPaths.push(card.dataset.fieldPath||'');}continue;}const current=normalizeFieldValue(control);const initial=initialFieldValue(control);const changed=!valuesEqual(current,initial);const path=card.dataset.fieldPath||control.dataset.fieldPath||'';const reason=fieldReasonSelect(card)?.value||'';const note=(fieldNoteInput(card)?.value||'').trim();if(changed){fieldEdits.push({path,value:current,reason:reason||null,note:note||null,origin:'local_app.fa_workbench'});if(reason){annotations.push({path,reason,note:note||null});}}if(fieldConfirmInput(card)?.checked){confirmedReviewPaths.push(path);}}return {base_version_id:reviewSessionState.currentDraftVersionId,actor_role:'financial_administrator',label:'FA saved revision',field_edits:fieldEdits,confirmed_review_paths:[...new Set(confirmedReviewPaths.filter(Boolean))],annotations};}\
         function setWorkbenchStatus(message,tone){const target=document.getElementById('workbench-status');if(!target){return;}target.textContent=message;target.dataset.tone=tone;}\
         async function saveReviewChanges(){if(reviewSessionState.saveInFlight){return;}const payload=buildRevisionPayload();if(payload.field_edits.length===0&&payload.confirmed_review_paths.length===0){setWorkbenchStatus('No review changes to save.','neutral');return;}reviewSessionState.saveInFlight=true;setWorkbenchStatus('Saving review changes and recomputing readiness…','saving');refreshDirtySummary();try{const response=await fetch('review-session/save',{method:'POST',headers:{'Content-Type':'application/json','Accept':'application/json'},body:JSON.stringify(payload)});const result=await response.json();if(!response.ok){throw new Error(result.error||`save failed (${response.status})`);}sessionStorage.setItem('reviewWorkbenchFlash',`Saved review revision v${result.version_id}. Readiness recomputed.`);window.location.reload();}catch(err){setWorkbenchStatus(`Save failed: ${err.message}`,'error');reviewSessionState.saveInFlight=false;refreshDirtySummary();}}\
         function initializeReviewWorkbench(){for(const control of document.querySelectorAll('.field-control, .structured-row-input')){control.addEventListener('input',()=>syncCardStateFromEventTarget(control));control.addEventListener('change',()=>syncCardStateFromEventTarget(control));}for(const editor of document.querySelectorAll('.structured-list-editor')){const rows=editor.querySelector('.structured-list-rows');const initial=parseJsonData(editor.dataset.initialJson,[]);if(rows&&rows.children.length===0&&Array.isArray(initial)){for(const rowValues of initial){rows.appendChild(structuredRowTemplate(editor,rowValues));}}}for(const card of document.querySelectorAll('.field-card')){updateFieldDirtyState(card);}refreshDirtySummary();loadReviewSessionSummary();const flash=sessionStorage.getItem('reviewWorkbenchFlash');if(flash){setWorkbenchStatus(flash,'success');sessionStorage.removeItem('reviewWorkbenchFlash');}document.getElementById('save-review-button')?.addEventListener('click',saveReviewChanges);document.getElementById('reset-review-button')?.addEventListener('click',resetReviewForm);document.getElementById('reload-review-button')?.addEventListener('click',()=>window.location.reload());}\
         function hasDirtyFields(){return document.querySelectorAll('.field-card.dirty').length>0;}\
         window.addEventListener('beforeunload',function(event){if(hasDirtyFields()&&!reviewSessionState.saveInFlight){event.preventDefault();event.returnValue='';}});\
         document.addEventListener('keydown',function(event){if((event.metaKey||event.ctrlKey)&&event.key==='s'){event.preventDefault();saveReviewChanges();}});\
         document.addEventListener('DOMContentLoaded',initializeReviewWorkbench);",
    );
    html.push_str("\n</script>\n</head>\n<body>\n<div class=\"shell\">");
    render_header(&mut html, packet);
    render_toolbar(&mut html);
    html.push_str("<main class=\"layout\">");
    render_issues_panel(&mut html, packet, &index);
    render_editor_panel(&mut html, packet, &index);
    html.push_str("</main>");
    render_source_documents_panel(&mut html, packet);
    html.push_str("</div></body></html>");
    html
}

fn render_header(html: &mut String, packet: &ReviewPacket) {
    html.push_str("<header class=\"hero\"><div class=\"hero-grid\"><div>");
    html.push_str("<p class=\"eyebrow\">FA Filing Surface</p><h1>Expense Report Workbench</h1>");
    html.push_str("<p class=\"hero-status\">");
    html.push_str(filing_status_label(packet.summary.filing_status));
    html.push_str("</p><p class=\"hero-subtitle\">");
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
    html.push_str("</p><p class=\"hero-copy\">Resolve the fields below, save the reviewed draft, and then use the final preview page for print/PDF export.</p></div>");
    html.push_str("<div class=\"summary-grid\">");
    summary_card(
        html,
        "Trip Window",
        packet.summary.trip_window.as_deref().unwrap_or("[missing]"),
    );
    summary_card(
        html,
        "Report Total USD",
        packet.summary.report_total_usd.as_deref().unwrap_or("[missing]"),
    );
    summary_card(
        html,
        "Category",
        packet.summary.category.as_deref().unwrap_or("[missing]"),
    );
    summary_card(
        html,
        "Transaction Type",
        packet
            .summary
            .transaction_type
            .as_deref()
            .unwrap_or("[missing]"),
    );
    html.push_str("</div></div></header>");
}

fn render_toolbar(html: &mut String) {
    html.push_str("<section class=\"toolbar\">");
    html.push_str("<div class=\"toolbar-main\"><div><p class=\"eyebrow\">Review Session</p><h2>Editable filing surface</h2><p class=\"toolbar-meta\">Current draft <span id=\"current-draft-version\">v?</span> · filing status <span id=\"current-filing-status\">loading…</span></p></div>");
    html.push_str("<div class=\"toolbar-actions\">");
    html.push_str("<button class=\"primary-button\" id=\"save-review-button\" type=\"button\">Save and recompute</button>");
    html.push_str("<button class=\"secondary-button\" id=\"reset-review-button\" type=\"button\">Reset unsaved changes</button>");
    html.push_str("<button class=\"ghost-button\" id=\"reload-review-button\" type=\"button\">Reload</button>");
    html.push_str("</div></div>");
    html.push_str("<div class=\"toolbar-subrow\"><p class=\"toolbar-status\" id=\"workbench-status\" data-tone=\"neutral\">Fill the missing values, adjust any fields that need review, then save to create a reviewed draft version.</p><p class=\"toolbar-count\" id=\"pending-change-count\">0 changes pending</p></div>");
    html.push_str("<div class=\"toolbar-links\">");
    html.push_str("<a class=\"nav-link\" href=\"preview\">Open final preview</a>");
    html.push_str("<a class=\"nav-link\" href=\"overview\">Bundle overview</a>");
    html.push_str("<a class=\"nav-link\" href=\"artifact/draft.yaml\" target=\"_blank\" rel=\"noreferrer\">Draft YAML</a>");
    html.push_str("<a class=\"nav-link\" href=\"review-session\" target=\"_blank\" rel=\"noreferrer\">Session summary</a>");
    html.push_str("</div></section>");
}

fn render_issues_panel(html: &mut String, packet: &ReviewPacket, index: &WorkbenchIndex) {
    html.push_str("<aside class=\"issues-panel\"><p class=\"eyebrow\">Action Queue</p><h2>What still needs attention</h2>");
    if packet.issues_queue.is_empty() {
        html.push_str("<p class=\"field-guidance\">No blocking or review items remain in the current packet.</p>");
    } else {
        html.push_str("<div class=\"issue-list\">");
        for issue in &packet.issues_queue {
            let target = issue_target(issue.path.as_str(), index);
            html.push_str("<article class=\"issue-card\"><div class=\"issue-head\"><div><span class=\"issue-class ");
            html.push_str(issue_badge_class(issue.class));
            html.push_str("\">");
            html.push_str(issue_label(issue.class));
            html.push_str("</span><p class=\"issue-label\">");
            html.push_str(&escape_html(&issue.label));
            html.push_str("</p></div>");
            if let Some(target) = target {
                html.push_str("<a class=\"issue-link\" href=\"#");
                html.push_str(&escape_html(&target));
                html.push_str("\" onclick=\"jumpToField(event, '");
                html.push_str(&escape_html_attribute(&target));
                html.push_str("')\">Jump to field</a>");
            }
            html.push_str("</div><p class=\"issue-message\">");
            html.push_str(&escape_html(&issue.message));
            html.push_str("</p></article>");
        }
        html.push_str("</div>");
    }
    html.push_str("</aside>");
}

fn render_editor_panel(html: &mut String, packet: &ReviewPacket, index: &WorkbenchIndex) {
    html.push_str("<section class=\"editor-panel\"><div class=\"editor-header\"><div><p class=\"eyebrow\">Filing Form</p><h2>Sections to complete</h2></div><p class=\"field-hint\">Use the queue on the left to jump to open items. Missing fields need your input, “Check this field” means a value exists but should be confirmed, and computed fields are system-generated.</p></div>");
    html.push_str("<div class=\"section-stack\">");
    for section in &packet.copy_sections {
        render_section(html, section, index);
    }
    html.push_str("</div></section>");
}

fn render_section(html: &mut String, section: &CopySection, index: &WorkbenchIndex) {
    let section_id = anchor_id("section", &section.key);
    html.push_str("<article class=\"section-card\" id=\"");
    html.push_str(&escape_html(&section_id));
    html.push_str("\"><p class=\"eyebrow\">Form section</p><h3>");
    html.push_str(&escape_html(&section.label));
    html.push_str("</h3><div class=\"instance-stack\">");
    for instance in &section.instances {
        let instance_id = anchor_id("instance", &instance.path);
        html.push_str("<section class=\"instance-card\" id=\"");
        html.push_str(&escape_html(&instance_id));
        html.push_str("\">");
        if section.repeated {
            html.push_str("<div class=\"field-head\"><div><h4>");
            html.push_str(&escape_html(&instance.label));
            html.push_str("</h4></div></div>");
        }
        html.push_str("<div class=\"field-list\">");
        for field in &instance.fields {
            render_field(html, field, index);
        }
        html.push_str("</div></section>");
    }
    html.push_str("</div></article>");
}

fn render_field(html: &mut String, field: &CopyField, index: &WorkbenchIndex) {
    let field_id = index
        .field_targets
        .get(&field.path)
        .cloned()
        .unwrap_or_else(|| anchor_id("field", &field.path));
    let input_id = format!("{field_id}-input");
    html.push_str("<article class=\"field-card");
    if !field.present {
        html.push_str(" missing");
    }
    if field_is_readonly(field) {
        html.push_str(" readonly");
    } else {
        html.push_str(" editable");
    }
    html.push_str("\" id=\"");
    html.push_str(&escape_html(&field_id));
    html.push_str("\" data-field-path=\"");
    html.push_str(&escape_html_attribute(&field.path));
    html.push_str("\">");
    html.push_str("<div class=\"field-head\"><div><p class=\"field-label\">");
    html.push_str(&escape_html(&field.label));
    html.push_str("</p></div><div class=\"field-badges\">");
    html.push_str("<span class=\"badge field-dirty-badge\" hidden>edited</span>");
    html.push_str("<span class=\"badge ");
    html.push_str(field_status_class(field));
    html.push_str("\">");
    html.push_str(field_status_label(field));
    html.push_str("</span>");
    if field.required {
        html.push_str("<span class=\"badge\">required</span>");
    }
    html.push_str("</div></div>");
    render_field_editor(html, field, &input_id);
    html.push_str("<p class=\"field-guidance\">");
    html.push_str(&escape_html(field_guidance(field)));
    html.push_str("</p>");
    render_review_controls(html, field);
    render_inline_evidence(html, field);
    html.push_str("</article>");
}

fn render_field_editor(html: &mut String, field: &CopyField, input_id: &str) {
    let value = field.value.as_deref().unwrap_or("");
    let placeholder = if field.present {
        String::new()
    } else {
        field_placeholder(field)
    };
    let readonly = if field_is_readonly(field) { " readonly" } else { "" };
    let disabled = if field_is_readonly(field) { " disabled" } else { "" };

    html.push_str("<div class=\"field-editor\"><label class=\"field-editor-label\" for=\"");
    html.push_str(&escape_html(input_id));
    html.push_str("\">");
    html.push_str(if field_is_readonly(field) {
        "Review computed value"
    } else if field.present {
        "Check or edit value"
    } else {
        "Add this value"
    });
    html.push_str("</label>");

    if field.control == FieldControl::StructuredList {
        render_structured_list_editor(html, field, input_id);
    } else if field.control == FieldControl::Textarea {
        html.push_str("<textarea class=\"field-input field-control\" id=\"");
        html.push_str(&escape_html(input_id));
        html.push_str("\" data-control=\"textarea\" data-initial-json=\"");
        html.push_str(&escape_html_attribute(&scalar_initial_json(value)));
        html.push_str("\" data-field-path=\"");
        html.push_str(&escape_html_attribute(&field.path));
        html.push_str("\" placeholder=\"");
        html.push_str(&escape_html_attribute(&placeholder));
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
        html.push_str("><option value=\"\">");
        html.push_str(&escape_html(if field.present {
            "Select a different option"
        } else {
            "Choose an option"
        }));
        html.push_str("</option>");
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
        html.push_str("<select class=\"field-input field-control\" id=\"");
        html.push_str(&escape_html(input_id));
        html.push_str("\" data-control=\"checkbox\" data-field-path=\"");
        html.push_str(&escape_html_attribute(&field.path));
        html.push_str("\" data-initial-json=\"");
        html.push_str(&escape_html_attribute(&bool_initial_json(field.value.as_deref())));
        html.push_str("\"");
        html.push_str(disabled);
        html.push_str(">");
        render_checkbox_option(
            html,
            "",
            if field.present {
                "Choose yes or no"
            } else {
                "Select yes or no"
            },
            value.is_empty(),
        );
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
        html.push_str(&escape_html_attribute(&placeholder));
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
    html.push_str("\"><p class=\"structured-list-copy\">");
    html.push_str(if field.present {
        "Edit the repeated rows directly. Add or remove rows as needed."
    } else {
        "Add one or more rows to supply this missing repeated field."
    });
    html.push_str("</p><div class=\"structured-list-rows\">");
    for row in &field.collection_rows {
        render_structured_list_row(html, &field.collection_columns, &row.values);
    }
    html.push_str("</div>");
    if !field_is_readonly(field) {
        html.push_str("<div class=\"field-actions\"><button class=\"secondary-button\" type=\"button\" onclick=\"addStructuredListRow(this)\">Add row</button></div>");
    }
    html.push_str("</div>");
}

fn render_structured_list_row(
    html: &mut String,
    columns: &[CopyCollectionColumn],
    values: &BTreeMap<String, String>,
) {
    html.push_str("<div class=\"structured-row\">");
    for column in columns {
        html.push_str("<label class=\"structured-cell\"><span class=\"structured-cell-label\">");
        html.push_str(&escape_html(&column.label));
        html.push_str("</span>");
        render_collection_column_control(
            html,
            column,
            values.get(&column.key).map(String::as_str).unwrap_or(""),
        );
        html.push_str("</label>");
    }
    html.push_str("<button class=\"structured-row-remove\" type=\"button\">Remove row</button></div>");
}

fn render_collection_column_control(
    html: &mut String,
    column: &CopyCollectionColumn,
    value: &str,
) {
    match column.control {
        FieldControl::Select => {
            html.push_str("<select class=\"structured-row-input\" data-column-key=\"");
            html.push_str(&escape_html_attribute(&column.key));
            html.push_str("\" data-column-control=\"select\"><option value=\"\"></option>");
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
            html.push_str("<select class=\"structured-row-input\" data-column-key=\"");
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
    html.push_str("<details class=\"field-review-controls\"><summary>Review notes</summary>");
    html.push_str("<label class=\"field-hint\"><input class=\"field-confirm-input\" type=\"checkbox\" onchange=\"syncCardStateFromEventTarget(this)\"> Mark this field reviewed</label>");
    if !field_is_readonly(field) {
        html.push_str("<div class=\"evidence-list\">");
        html.push_str("<label class=\"field-hint\">Correction reason<select class=\"field-reason-select\" onchange=\"syncCardStateFromEventTarget(this)\"><option value=\"\"></option>");
        for (value, label) in correction_reason_options() {
            html.push_str("<option value=\"");
            html.push_str(value);
            html.push_str("\">");
            html.push_str(label);
            html.push_str("</option>");
        }
        html.push_str("</select></label>");
        html.push_str("<label class=\"field-hint\">Review note<textarea class=\"field-note-input\" placeholder=\"Optional note for the ledger and feedback history\" oninput=\"syncCardStateFromEventTarget(this)\"></textarea></label></div>");
    }
    html.push_str("</details>");
}

fn render_inline_evidence(html: &mut String, field: &CopyField) {
    if field.evidence.is_empty() {
        return;
    }
    html.push_str("<details class=\"field-evidence\"><summary>Evidence (");
    html.push_str(&field.evidence.len().to_string());
    html.push_str(")</summary><div class=\"evidence-list\">");
    for evidence in &field.evidence {
        html.push_str("<article class=\"evidence-card\"><p class=\"field-label\">");
        html.push_str(&escape_html(&evidence_title(evidence)));
        html.push_str("</p>");
        if let Some(source) = evidence_source_label(evidence).as_deref() {
            html.push_str("<p class=\"evidence-detail\">Source: ");
            html.push_str(&escape_html(source));
            html.push_str("</p>");
        }
        if let Some(origin) = evidence.origin.as_deref() {
            html.push_str("<p class=\"evidence-detail\">Origin: ");
            html.push_str(&escape_html(origin));
            html.push_str("</p>");
        }
        if let Some(quote) = evidence.quote.as_deref() {
            html.push_str("<p class=\"evidence-copy\">");
            html.push_str(&escape_html(quote));
            html.push_str("</p>");
        }
        if let Some(document_href) = evidence_document_href(evidence) {
            html.push_str("<div class=\"evidence-actions\"><a class=\"doc-link\" href=\"");
            html.push_str(&escape_html_attribute(&document_href));
            html.push_str("\" target=\"_blank\" rel=\"noreferrer noopener\">Open source document</a></div>");
        }
        html.push_str("</article>");
    }
    html.push_str("</div></details>");
}

fn render_source_documents_panel(html: &mut String, packet: &ReviewPacket) {
    html.push_str("<section class=\"source-panel\"><div class=\"source-head\"><div><p class=\"eyebrow\">Source Documents</p><h2>Uploaded files</h2></div><p class=\"field-hint\">Open the original uploads directly when you need to confirm evidence or review a receipt manually.</p></div>");
    if packet.document_snapshots.is_empty() {
        html.push_str("<p class=\"field-guidance\">No uploaded documents are available in the current packet.</p>");
    } else {
        html.push_str("<div class=\"source-grid\">");
        for document in &packet.document_snapshots {
            html.push_str("<article class=\"source-card\"><div class=\"source-head\"><div><h4>");
            html.push_str(&escape_html(&document.filename));
            html.push_str("</h4><p class=\"source-status\">");
            html.push_str(&escape_html(&document.status_label));
            html.push_str("</p></div><span class=\"badge ");
            html.push_str(if document.projected_to_filing {
                "status-ready"
            } else if document.used_in_bundle {
                "status-review"
            } else {
                "status-computed"
            });
            html.push_str("\">");
            html.push_str(if document.projected_to_filing {
                "Used"
            } else if document.used_in_bundle {
                "Bundle only"
            } else {
                "Captured"
            });
            html.push_str("</span></div><p class=\"source-meta\">");
            html.push_str(&escape_html(&document.kind));
            html.push_str(" · ");
            html.push_str(&escape_html(&document.extraction_status));
            html.push_str("</p><div class=\"source-links\"><a class=\"doc-link\" href=\"document/");
            html.push_str(&escape_html_attribute(&document.document_id));
            html.push('/');
            html.push_str(&escape_html_attribute(&document.filename));
            html.push_str("\" target=\"_blank\" rel=\"noreferrer noopener\">Open source document</a></div></article>");
        }
        html.push_str("</div>");
    }
    html.push_str("</section>");
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
                index
                    .field_targets
                    .insert(field.path.clone(), anchor_id("field", &field.path));
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
    html.push_str("</p></article>");
}

fn field_is_readonly(field: &CopyField) -> bool {
    field.entry_mode == FieldEntryMode::ComputedReadonly
}

fn field_status_label(field: &CopyField) -> &'static str {
    if !field.present && field.required {
        "Needs your input"
    } else if field.needs_review {
        "Check this field"
    } else if field_is_readonly(field) {
        "Computed"
    } else {
        "Filled in"
    }
}

fn field_status_class(field: &CopyField) -> &'static str {
    if !field.present && field.required {
        "status-missing"
    } else if field.needs_review {
        "status-review"
    } else if field_is_readonly(field) {
        "status-computed"
    } else {
        "status-ready"
    }
}

fn issue_label(class: crate::ReadinessIssueClass) -> &'static str {
    match class {
        crate::ReadinessIssueClass::AutomationGap => "Automation gap",
        crate::ReadinessIssueClass::UserInputRequired => "Needs your input",
        crate::ReadinessIssueClass::ManualReview => "Check this field",
        crate::ReadinessIssueClass::OtherWarning => "Warning",
    }
}

fn issue_badge_class(class: crate::ReadinessIssueClass) -> &'static str {
    match class {
        crate::ReadinessIssueClass::AutomationGap => "missing",
        crate::ReadinessIssueClass::UserInputRequired => "missing",
        crate::ReadinessIssueClass::ManualReview => "review",
        crate::ReadinessIssueClass::OtherWarning => "ready",
    }
}

fn field_guidance(field: &CopyField) -> &'static str {
    if field_is_readonly(field) {
        "This field is computed by the system. Review it, but only override it if the filing workflow requires a manual correction."
    } else if field.present {
        "A value is already present. Confirm it or edit it directly if the current value is incomplete or incorrect."
    } else {
        "This field is currently missing. Enter the value here so the filing packet can move forward."
    }
}

fn field_placeholder(field: &CopyField) -> String {
    let label = field.label.trim();
    if field.required {
        format!("Enter {label}")
    } else {
        format!("Optional: {label}")
    }
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
            .clone()
            .unwrap_or_else(|| "Derived value".to_owned()),
        crate::draft::EvidenceKind::UserInput => "User-provided value".to_owned(),
    }
}

fn evidence_source_label(evidence: &EvidenceReference) -> Option<String> {
    evidence
        .filename
        .clone()
        .or_else(|| evidence.document_id.clone())
}

fn evidence_document_href(evidence: &EvidenceReference) -> Option<String> {
    let document_id = evidence.document_id.as_deref()?;
    let filename = evidence
        .filename
        .as_deref()
        .or(evidence.document_id.as_deref())?;
    Some(format!("document/{document_id}/{filename}"))
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
        ("wrong_document_classification", "Wrong document classification"),
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
        ("unclear_or_undocumented_rule", "Unclear or undocumented rule"),
        ("other", "Other"),
    ]
}

fn filing_status_label(status: FilingStatus) -> &'static str {
    match status {
        FilingStatus::AutomationBlocked => "automation blocked",
        FilingStatus::UserInputRequired => "user input required",
        FilingStatus::ManualReviewRequired => "manual review required",
        FilingStatus::ReadyToFile => "ready to file",
    }
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
    use super::render_fa_workbench_html;
    use crate::bundle_synthesis::synthesize_bundle_projection_with_fx;
    use crate::review_packet::build_review_packet;
    use crate::synthetic_documents::{generate_synthetic_packet, SyntheticVariant};
    use crate::StaticFxRateProvider;

    fn synthetic_packet() -> crate::ReviewPacket {
        let documents = generate_synthetic_packet(SyntheticVariant::Baseline)
            .into_iter()
            .map(|fixture| fixture.expected_facts)
            .collect::<Vec<_>>();
        let projection =
            synthesize_bundle_projection_with_fx(&documents, &StaticFxRateProvider::demo());
        build_review_packet(&projection.bundle, &projection.draft, &projection.validation)
            .expect("review packet should build")
    }

    #[test]
    fn fa_workbench_is_editable_and_preview_linked() {
        let rendered = render_fa_workbench_html(&synthetic_packet());
        assert!(rendered.contains("Open final preview"));
        assert!(rendered.contains("Save and recompute"));
        assert!(rendered.contains("Add this value"));
        assert!(rendered.contains("Choose an option"));
        assert!(rendered.contains("Check or edit value"));
        assert!(rendered.contains("Jump to field"));
        assert!(rendered.contains("beforeunload"));
    }

    #[test]
    fn fa_workbench_omits_developer_specific_ocr_debug_sections() {
        let rendered = render_fa_workbench_html(&synthetic_packet());
        assert!(!rendered.contains("OCR And Extraction Snapshot"));
        assert!(!rendered.contains("Open OCR diff"));
        assert!(!rendered.contains("Open OCR inspection"));
        assert!(!rendered.contains("Fields To Double-Check"));
    }

    #[test]
    fn fa_workbench_hides_schema_paths_and_uses_friendlier_labels() {
        let rendered = render_fa_workbench_html(&synthetic_packet());
        assert!(!rendered.contains("class=\"field-path\""));
        assert!(!rendered.contains("class=\"issue-path\""));
        assert!(rendered.contains("Payee Name"));
        assert!(rendered.contains("Payee Affiliation"));
    }
}
