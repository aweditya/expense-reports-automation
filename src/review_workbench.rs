use std::collections::BTreeMap;

use crate::draft::EvidenceReference;
use crate::review_packet::{
    CopyField, DocumentSnapshotCard, DocumentSnapshotField, FilingStatus, ReviewPacket,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct WorkbenchIndex {
    field_targets: BTreeMap<String, String>,
    instance_targets: BTreeMap<String, String>,
    section_targets: BTreeMap<String, String>,
}

pub fn render_review_workbench_html(packet: &ReviewPacket) -> String {
    let index = build_workbench_index(packet);
    let mut html = String::new();

    html.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    html.push_str(
        "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>Expense Report Review Workbench</title>\n",
    );
    html.push_str("<style>\n");
    html.push_str(include_str!("review_workbench.css"));
    html.push_str("\n</style>\n");
    html.push_str(
        "<script>\nconst reviewSessionState={currentDraftVersionId:null,saveInFlight:false};\nfunction fieldControl(card){return card?card.querySelector('.field-control'):null;}\nfunction issueCountLabel(count){return count===1?'1 change pending':`${count} changes pending`;}\nfunction openAncestorDetails(target){let current=target?.parentElement;while(current){if(current.tagName==='DETAILS'){current.open=true;}current=current.parentElement;}}\nfunction revealHashTarget(){const rawHash=window.location.hash||'';if(!rawHash||rawHash==='#'){return;}const target=document.getElementById(rawHash.slice(1));if(!target){return;}openAncestorDetails(target);target.scrollIntoView({behavior:'smooth',block:'center'});const focusTarget=target.matches('.field-card')?target.querySelector('.field-control, .structured-row-input'):target.querySelector?.('.field-control, .structured-row-input');if(focusTarget){focusTarget.focus();if(typeof focusTarget.select==='function'){focusTarget.select();}}}\nfunction jumpToField(event,targetId){const target=document.getElementById(targetId);if(!target){return;}event.preventDefault();openAncestorDetails(target);target.scrollIntoView({behavior:'smooth',block:'center'});window.location.hash=targetId;const focusTarget=target.querySelector('.field-control, .structured-row-input');if(focusTarget){focusTarget.focus();if(typeof focusTarget.select==='function'){focusTarget.select();}}}\nfunction flashCopy(button){button.textContent='Copied';setTimeout(()=>{button.textContent='Copy';},900);}\nfunction structuredRowsPayload(editor){if(!editor){return [];}const rows=[];for(const row of editor.querySelectorAll('.structured-row')){const values={};let hasAnyValue=false;for(const input of row.querySelectorAll('.structured-row-input')){const key=input.dataset.columnKey||'';let value=('value' in input)?input.value:'';if(input.dataset.columnControl==='checkbox'){value=input.value;}if(value!==''&&value!==null){hasAnyValue=true;}values[key]=value;}if(hasAnyValue){rows.push(values);}}return rows;}\nfunction valueForCopy(card){const control=fieldControl(card);if(!control){return '';}if(control.classList.contains('structured-list-editor')){const rows=structuredRowsPayload(control);if(rows.length===0){return '';}return rows.map((row,index)=>`${index+1}. `+Object.entries(row).filter(([,value])=>value!==''&&value!==null).map(([key,value])=>`${key}: ${value}`).join(' | ')).join('\\n');}if(control.tagName==='SELECT'){return control.value||'';}if('value' in control){return control.value||'';}return '';} \nfunction copyFieldValue(button){const card=button.closest('.field-card');if(!card){return;}const value=valueForCopy(card);if(!value){return;}navigator.clipboard.writeText(value);flashCopy(button);} \nfunction buildDocumentPreviewUrl(url,page){if(!page){return url;}if(/\\.pdf(?:$|[?#])/i.test(url)){const separator=url.includes('#')?'&':'#';return `${url}${separator}page=${page}`;}return url;}\nfunction openDocumentPreview(url,title,page,quote,originLabel){const backdrop=document.getElementById('document-preview-backdrop');const drawer=document.getElementById('document-preview-drawer');const frame=document.getElementById('document-preview-frame');const label=document.getElementById('document-preview-title');const meta=document.getElementById('document-preview-meta');const excerpt=document.getElementById('document-preview-excerpt');const popout=document.getElementById('document-preview-open');if(!drawer||!frame||!label||!meta||!excerpt){return;}const previewUrl=buildDocumentPreviewUrl(url,page);frame.src=previewUrl;label.textContent=title||'Source document';meta.textContent=[page?`page ${page}`:null,originLabel||null].filter(Boolean).join(' · ');excerpt.textContent=quote||'No excerpt captured for this evidence reference.';if(popout){popout.href=previewUrl;popout.hidden=false;}if(backdrop){backdrop.hidden=false;}drawer.hidden=false;drawer.setAttribute('aria-hidden','false');drawer.focus();if(typeof drawer.scrollTo==='function'){drawer.scrollTo({top:0,left:0,behavior:'auto'});}document.body.classList.add('document-open');}\nfunction closeDocumentPreview(){const backdrop=document.getElementById('document-preview-backdrop');const drawer=document.getElementById('document-preview-drawer');const frame=document.getElementById('document-preview-frame');const popout=document.getElementById('document-preview-open');if(!drawer||!frame){return;}drawer.hidden=true;drawer.setAttribute('aria-hidden','true');if(backdrop){backdrop.hidden=true;}frame.src='about:blank';if(popout){popout.href='#';popout.hidden=true;}document.body.classList.remove('document-open');}\nfunction parseJsonData(value,fallback){if(!value){return fallback;}try{return JSON.parse(value);}catch(_err){return fallback;}}\nfunction normalizeFieldValue(control){if(!control){return null;}if(control.classList.contains('structured-list-editor')){const columns=parseJsonData(control.dataset.columnsJson,'[]');const rows=structuredRowsPayload(control).map((row)=>{const obj={};for(const column of columns){const raw=row[column.key]??'';if(raw===''){continue;}if(column.control==='checkbox'){obj[column.key]=raw==='true';}else{obj[column.key]=raw;}}return obj;});return rows.length===0?[]:rows;}if(control.dataset.control==='checkbox'){if(control.value===''){return null;}return control.value==='true';}if('value' in control){return control.value===''?null:control.value;}return null;}\nfunction initialFieldValue(control){if(!control){return null;}const fallback=control.classList.contains('structured-list-editor')?[]:null;return parseJsonData(control.dataset.initialJson,fallback);} \nfunction valuesEqual(left,right){return JSON.stringify(left)===JSON.stringify(right);} \nfunction fieldReasonSelect(card){return card.querySelector('.field-reason-select');}\nfunction fieldNoteInput(card){return card.querySelector('.field-note-input');}\nfunction fieldConfirmInput(card){return card.querySelector('.field-confirm-input');}\nfunction updateFieldDirtyState(card){const control=fieldControl(card);if(!control){return;}const edited=!valuesEqual(initialFieldValue(control),normalizeFieldValue(control));const confirmed=Boolean(fieldConfirmInput(card)?.checked);card.classList.toggle('dirty',edited||confirmed);const badge=card.querySelector('.field-dirty-badge');if(badge){badge.hidden=!(edited||confirmed);}}\nfunction refreshDirtySummary(){const dirtyCards=[...document.querySelectorAll('.field-card.dirty')];const counter=document.getElementById('pending-change-count');if(counter){counter.textContent=issueCountLabel(dirtyCards.length);}const saveButton=document.getElementById('save-review-button');const resetButton=document.getElementById('reset-review-button');if(saveButton){saveButton.disabled=reviewSessionState.saveInFlight||dirtyCards.length===0;}if(resetButton){resetButton.disabled=reviewSessionState.saveInFlight||dirtyCards.length===0;}}\nfunction syncCardStateFromEventTarget(target){const card=target.closest('.field-card');if(!card){return;}updateFieldDirtyState(card);refreshDirtySummary();}\nfunction structuredRowTemplate(editor,rowValues){const columns=parseJsonData(editor.dataset.columnsJson,[]);const row=document.createElement('div');row.className='structured-row';for(const column of columns){const cell=document.createElement('label');cell.className='structured-cell';const label=document.createElement('span');label.className='structured-cell-label';label.textContent=column.label;cell.appendChild(label);let input;if(column.control==='select'){input=document.createElement('select');const blank=document.createElement('option');blank.value='';blank.textContent='';input.appendChild(blank);for(const optionValue of column.allowed_values||[]){const option=document.createElement('option');option.value=optionValue;option.textContent=optionValue.replaceAll('_',' ');input.appendChild(option);}}else if(column.control==='checkbox'){input=document.createElement('select');[['','Unset'],['true','Yes'],['false','No']].forEach(([value,labelText])=>{const option=document.createElement('option');option.value=value;option.textContent=labelText;input.appendChild(option);});}else if(column.control==='date'){input=document.createElement('input');input.type='date';}else{input=document.createElement(column.control==='textarea'?'textarea':'input');if(input.tagName==='INPUT'){input.type='text';if(column.control==='currency'||column.control==='number'){input.inputMode='decimal';}}}input.className='structured-row-input';input.dataset.columnKey=column.key;input.dataset.columnControl=column.control;input.value=(rowValues&&rowValues[column.key])||'';input.addEventListener('input',()=>syncCardStateFromEventTarget(input));input.addEventListener('change',()=>syncCardStateFromEventTarget(input));cell.appendChild(input);row.appendChild(cell);}const removeButton=document.createElement('button');removeButton.type='button';removeButton.className='structured-row-remove';removeButton.textContent='Remove row';removeButton.addEventListener('click',()=>{row.remove();syncCardStateFromEventTarget(editor);});row.appendChild(removeButton);return row;}\nfunction addStructuredListRow(button){const editor=button.closest('.structured-list-editor');if(!editor){return;}const rows=editor.querySelector('.structured-list-rows');if(!rows){return;}rows.appendChild(structuredRowTemplate(editor,{}));syncCardStateFromEventTarget(editor);} \nfunction resetReviewForm(){for(const card of document.querySelectorAll('.field-card')){const control=fieldControl(card);if(!control){continue;}const initial=initialFieldValue(control);if(control.classList.contains('structured-list-editor')){const rows=control.querySelector('.structured-list-rows');if(rows){rows.innerHTML='';for(const rowValues of Array.isArray(initial)?initial:[]){rows.appendChild(structuredRowTemplate(control,rowValues));}}}else if(control.dataset.control==='checkbox'){control.value=initial===null?'':String(initial);}else if('value' in control){control.value=initial??'';}const reason=fieldReasonSelect(card);const note=fieldNoteInput(card);const confirm=fieldConfirmInput(card);if(reason){reason.value='';}if(note){note.value='';}if(confirm){confirm.checked=false;}updateFieldDirtyState(card);}setWorkbenchStatus('Unsaved review edits cleared.','neutral');refreshDirtySummary();revealHashTarget();}\nasync function loadReviewSessionSummary(){try{const response=await fetch('review-session',{headers:{'Accept':'application/json'}});if(!response.ok){throw new Error(`session lookup failed (${response.status})`);}const payload=await response.json();reviewSessionState.currentDraftVersionId=payload.current_draft_version_id;const versionLabel=document.getElementById('current-draft-version');if(versionLabel){versionLabel.textContent=`v${payload.current_draft_version_id}`;}const filingStatus=document.getElementById('current-filing-status');if(filingStatus){filingStatus.textContent=payload.filing_status.replaceAll('_',' ');} }catch(err){setWorkbenchStatus(`Unable to load review session metadata: ${err.message}`,'error');}}\nfunction buildRevisionPayload(){const fieldEdits=[];const confirmedReviewPaths=[];const annotations=[];for(const card of document.querySelectorAll('.field-card')){const control=fieldControl(card);if(!control||card.classList.contains('readonly')){if(fieldConfirmInput(card)?.checked){confirmedReviewPaths.push(card.dataset.fieldPath||'');}continue;}const current=normalizeFieldValue(control);const initial=initialFieldValue(control);const changed=!valuesEqual(current,initial);const path=card.dataset.fieldPath||control.dataset.fieldPath||'';const reason=fieldReasonSelect(card)?.value||'';const note=(fieldNoteInput(card)?.value||'').trim();if(changed){fieldEdits.push({path,value:current,reason:reason||null,note:note||null,origin:'local_app.review_workbench'});if(reason){annotations.push({path,reason,note:note||null});}}if(fieldConfirmInput(card)?.checked){confirmedReviewPaths.push(path);}}\nreturn {base_version_id:reviewSessionState.currentDraftVersionId,actor_role:'financial_administrator',label:'FA saved revision',field_edits:fieldEdits,confirmed_review_paths:[...new Set(confirmedReviewPaths.filter(Boolean))],annotations};}\nfunction setWorkbenchStatus(message,tone){const target=document.getElementById('workbench-status');if(!target){return;}target.textContent=message;target.dataset.tone=tone;}\nasync function saveReviewChanges(){if(reviewSessionState.saveInFlight){return;}const payload=buildRevisionPayload();if(payload.field_edits.length===0&&payload.confirmed_review_paths.length===0){setWorkbenchStatus('No review changes to save.','neutral');return;}reviewSessionState.saveInFlight=true;setWorkbenchStatus('Saving review changes and recomputing readiness…','saving');refreshDirtySummary();try{const response=await fetch('review-session/save',{method:'POST',headers:{'Content-Type':'application/json','Accept':'application/json'},body:JSON.stringify(payload)});const result=await response.json();if(!response.ok){throw new Error(result.error||`save failed (${response.status})`);}sessionStorage.setItem('reviewWorkbenchFlash',`Saved review revision v${result.version_id}. Readiness recomputed.`);window.location.reload();}catch(err){setWorkbenchStatus(`Save failed: ${err.message}`,'error');reviewSessionState.saveInFlight=false;refreshDirtySummary();}}\nfunction initializeReviewWorkbench(){for(const control of document.querySelectorAll('.field-control, .structured-row-input')){control.addEventListener('input',()=>syncCardStateFromEventTarget(control));control.addEventListener('change',()=>syncCardStateFromEventTarget(control));}\nfor(const editor of document.querySelectorAll('.structured-list-editor')){const rows=editor.querySelector('.structured-list-rows');const initial=parseJsonData(editor.dataset.initialJson,[]);if(rows&&rows.children.length===0&&Array.isArray(initial)){for(const rowValues of initial){rows.appendChild(structuredRowTemplate(editor,rowValues));}}}\nfor(const card of document.querySelectorAll('.field-card')){updateFieldDirtyState(card);}refreshDirtySummary();loadReviewSessionSummary();const flash=sessionStorage.getItem('reviewWorkbenchFlash');if(flash){setWorkbenchStatus(flash,'success');sessionStorage.removeItem('reviewWorkbenchFlash');}const saveButton=document.getElementById('save-review-button');const resetButton=document.getElementById('reset-review-button');const refreshButton=document.getElementById('reload-review-button');if(saveButton){saveButton.addEventListener('click',saveReviewChanges);}if(resetButton){resetButton.addEventListener('click',resetReviewForm);}if(refreshButton){refreshButton.addEventListener('click',()=>window.location.reload());}revealHashTarget();window.addEventListener('hashchange',revealHashTarget);}\ndocument.addEventListener('click',function(event){const closeTrigger=event.target.closest('[data-document-preview-close]');if(closeTrigger){event.preventDefault();closeDocumentPreview();return;}const link=event.target.closest('a[data-document-preview]');if(!link){return;}if(event.metaKey||event.ctrlKey||event.shiftKey||event.altKey){return;}event.preventDefault();openDocumentPreview(link.href,link.getAttribute('data-document-title')||link.textContent||'Source document',link.getAttribute('data-document-page'),link.getAttribute('data-document-quote')||'',link.getAttribute('data-document-origin')||'');});\ndocument.addEventListener('keydown',function(event){const drawer=document.getElementById('document-preview-drawer');if(event.key==='Escape'&&drawer&&!drawer.hidden){closeDocumentPreview();}});\ndocument.addEventListener('DOMContentLoaded',initializeReviewWorkbench);\n</script>\n",
    );
    html.push_str("</head>\n<body>\n<div class=\"shell\">\n");

    render_header(&mut html, packet);
    render_toolbar(&mut html);
    render_document_snapshot_panel(&mut html, packet);
    html.push_str("<main class=\"workbench-grid\">\n");
    render_issues_panel(&mut html, packet, &index);
    render_copy_panel(&mut html, packet, &index);
    html.push_str("</main>\n");
    render_attachments_panel(&mut html, packet);
    render_document_preview_drawer(&mut html);
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
    html.push_str("<a href=\"artifact/draft.yaml\" target=\"_blank\" rel=\"noreferrer\">Draft YAML</a>");
    html.push_str("<a href=\"artifact/review_packet.json\" target=\"_blank\" rel=\"noreferrer\">Review Packet JSON</a>");
    html.push_str("<a href=\"artifact/ledger.json\" target=\"_blank\" rel=\"noreferrer\">Ledger JSON</a>");
    html.push_str("<a href=\"review-session\" target=\"_blank\" rel=\"noreferrer\">Session Summary</a>");
    html.push_str("<a href=\"manifest\" target=\"_blank\" rel=\"noreferrer\">Bundle Manifest</a>");
    html.push_str("</div>");
    html.push_str("</section>\n");
}

fn render_issues_panel(html: &mut String, packet: &ReviewPacket, index: &WorkbenchIndex) {
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
    html.push_str("</section>\n");
}

fn render_copy_panel(html: &mut String, packet: &ReviewPacket, index: &WorkbenchIndex) {
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
                render_copy_field(html, field, index);
            }
            html.push_str("</div>\n</article>\n");
        }
        html.push_str("</section>\n");
    }
    html.push_str("</section>\n");
}

fn render_copy_field(html: &mut String, field: &CopyField, index: &WorkbenchIndex) {
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
    if field.control == "structured_list" {
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
    badge(html, &field.entry_mode);
    if field.required {
        badge(html, "required");
    }
    if field.needs_review {
        badge(html, "review");
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
    render_review_controls(html, field);
    render_inline_evidence(html, field);
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
    html.push_str(if field.present {
        "Review or edit value"
    } else {
        "Enter missing value"
    });
    html.push_str("</label>");

    if field.control == "structured_list" {
        render_structured_list_editor(html, field, input_id);
    } else if field.control == "textarea" {
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
    } else if field.control == "select" {
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
    } else if field.control == "checkbox" {
        html.push_str("<select class=\"field-input field-control checkbox-select\" id=\"");
        html.push_str(&escape_html(input_id));
        html.push_str("\" data-control=\"checkbox\" data-field-path=\"");
        html.push_str(&escape_html_attribute(&field.path));
        html.push_str("\" data-initial-json=\"");
        html.push_str(&escape_html_attribute(&bool_initial_json(field.value.as_deref())));
        html.push_str("\"");
        html.push_str(disabled);
        html.push_str(">");
        render_checkbox_option(html, "", "Unset", value.is_empty());
        render_checkbox_option(html, "true", "Yes", value == "true");
        render_checkbox_option(html, "false", "No", value == "false");
        html.push_str("</select>");
    } else {
        let input_type = if field.control == "date" { "date" } else { "text" };
        html.push_str("<input class=\"field-input field-control\" id=\"");
        html.push_str(&escape_html(input_id));
        html.push_str("\" type=\"");
        html.push_str(input_type);
        html.push_str("\" data-control=\"");
        html.push_str(&escape_html_attribute(&field.control));
        html.push_str("\" data-field-path=\"");
        html.push_str(&escape_html_attribute(&field.path));
        html.push_str("\" data-initial-json=\"");
        html.push_str(&escape_html_attribute(&scalar_initial_json(value)));
        html.push_str("\" value=\"");
        html.push_str(&escape_html_attribute(value));
        html.push_str("\" placeholder=\"");
        html.push_str(&escape_html_attribute(placeholder));
        html.push_str("\"");
        if field.control == "currency" || field.control == "number" {
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
    match column.control.as_str() {
        "select" => {
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
        "checkbox" => {
            html.push_str("<select class=\"structured-row-input checkbox-select\" data-column-key=\"");
            html.push_str(&escape_html_attribute(&column.key));
            html.push_str("\" data-column-control=\"checkbox\">");
            render_checkbox_option(html, "", "Unset", value.is_empty());
            render_checkbox_option(html, "true", "Yes", value == "true");
            render_checkbox_option(html, "false", "No", value == "false");
            html.push_str("</select>");
        }
        "date" => {
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
            html.push_str(&escape_html_attribute(&column.control));
            html.push_str("\" value=\"");
            html.push_str(&escape_html_attribute(value));
            html.push_str("\"");
            if column.control == "currency" || column.control == "number" {
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

fn render_inline_evidence(html: &mut String, field: &CopyField) {
    if field.evidence.is_empty() {
        return;
    }

    let details_id = anchor_id("field-evidence", &field.path);
    html.push_str("<details class=\"field-evidence\" id=\"");
    html.push_str(&escape_html(&details_id));
    html.push_str("\"><summary>");
    html.push_str(&escape_html(&format!("Evidence ({})", field.evidence.len())));
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
            html.push_str("<p class=\"evidence-detail\"><span class=\"evidence-detail-label\">Source</span>");
            html.push_str(&escape_html(source_label));
            html.push_str("</p>");
        }
        if let Some(origin_label) = evidence.origin.as_deref() {
            html.push_str("<p class=\"evidence-detail\"><span class=\"evidence-detail-label\">Origin</span>");
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
            let title = evidence
                .filename
                .as_deref()
                .or(evidence.document_id.as_deref())
                .unwrap_or("Source document");
            html.push_str("<div class=\"evidence-document-actions\">");
            html.push_str("<a class=\"document-link\" href=\"");
            html.push_str(&escape_html_attribute(&document_href));
            html.push_str("\" data-document-preview=\"true\" data-document-title=\"");
            html.push_str(&escape_html_attribute(title));
            html.push_str("\" data-document-origin=\"");
            html.push_str(&escape_html_attribute(
                evidence.origin.as_deref().unwrap_or("uploaded evidence"),
            ));
            html.push_str("\" data-document-page=\"");
            html.push_str(&escape_html_attribute(
                &evidence.page.map(|page| page.to_string()).unwrap_or_default(),
            ));
            html.push_str("\" data-document-quote=\"");
            html.push_str(&escape_html_attribute(evidence.quote.as_deref().unwrap_or("")));
            html.push_str("\">Open source document</a>");
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
        html.push_str("<p class=\"empty-state\">No source-document extraction snapshot is available.</p>\n");
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
    let document_href = format!("document/{}/{}", document.document_id, document.filename);
    html.push_str("<div class=\"document-snapshot-actions\">");
    html.push_str("<a class=\"document-link\" href=\"");
    html.push_str(&escape_html_attribute(&document_href));
    html.push_str("\" data-document-preview=\"true\" data-document-title=\"");
    html.push_str(&escape_html_attribute(&document.filename));
    html.push_str("\" data-document-origin=\"uploaded source document\" data-document-page=\"\" data-document-quote=\"\">Open source document</a>");
    html.push_str("</div>");
    html.push_str("</article>");
}

fn render_document_snapshot_field(html: &mut String, field: &DocumentSnapshotField) {
    html.push_str("<div class=\"document-snapshot-field\">");
    html.push_str("<dt>");
    html.push_str(&escape_html(&field.label));
    html.push_str("</dt>");
    html.push_str("<dd>");
    html.push_str(&escape_html(&field.value));
    html.push_str("</dd>");
    html.push_str("</div>");
}

fn render_document_preview_drawer(html: &mut String) {
    html.push_str("<button class=\"document-backdrop\" id=\"document-preview-backdrop\" type=\"button\" hidden data-document-preview-close=\"backdrop\" aria-label=\"Close source document preview\"></button>");
    html.push_str("<aside class=\"document-drawer\" id=\"document-preview-drawer\" hidden tabindex=\"-1\" role=\"dialog\" aria-modal=\"true\" aria-hidden=\"true\" aria-labelledby=\"document-preview-title\" aria-describedby=\"document-preview-excerpt\">");
    html.push_str("<div class=\"document-drawer-head\"><div><p class=\"eyebrow\">Source Document</p><h2 id=\"document-preview-title\">Source document</h2><p class=\"document-drawer-meta\" id=\"document-preview-meta\"></p></div>");
    html.push_str("<div class=\"document-drawer-actions\"><a class=\"document-popout-link\" id=\"document-preview-open\" href=\"#\" target=\"_blank\" rel=\"noreferrer noopener\" hidden>Open in new tab</a><button class=\"document-modal-close\" type=\"button\" data-document-preview-close=\"button\">Close preview</button></div></div>");
    html.push_str("<div class=\"document-drawer-summary\"><p class=\"document-drawer-label\">Captured Excerpt</p><blockquote id=\"document-preview-excerpt\">No excerpt captured for this evidence reference.</blockquote></div>");
    html.push_str("<iframe id=\"document-preview-frame\" title=\"Source document preview\" loading=\"lazy\"></iframe>");
    html.push_str("</aside>");
}

fn field_is_readonly(field: &CopyField) -> bool {
    field.entry_mode == "computed_readonly"
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
        crate::draft::EvidenceKind::Document | crate::draft::EvidenceKind::DocumentSpan => {
            evidence
                .filename
                .clone()
                .or_else(|| evidence.document_id.clone())
        }
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
    value.split('_')
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
        assert!(rendered.contains("document-preview-backdrop"));
        assert!(rendered.contains("document-preview-drawer"));
        assert!(rendered.contains("data-document-preview-close"));
        assert!(rendered.contains("Open in new tab"));
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
        assert!(rendered.contains("href=\"#field-expense-report-general-information-authorized-by\""));
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
        assert!(rendered.contains("data-document-quote="));
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
}
