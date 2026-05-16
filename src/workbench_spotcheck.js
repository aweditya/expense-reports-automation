// Spotcheck side panel (Phase 6 Stage A).
//
// When the FA clicks a "view source" button on an extracted field card,
// open the side panel and render the source document at the right page.
// Stage A is page navigation only — no highlighting yet (Stage B).
//
// Source-document URLs are constructed from `SOURCE_DOCS_URL_PREFIX`
// (set by Rust render to either "files/" for Cloud Run or
// "../../receipts/" for local-CLI rendering). The prefix is a single
// global so per-card emit doesn't need to know the deployment context.
(function() {
  'use strict';

  if (typeof window.pdfjsLib !== 'undefined') {
    window.pdfjsLib.GlobalWorkerOptions.workerSrc =
      'https://cdnjs.cloudflare.com/ajax/libs/pdf.js/3.11.174/pdf.worker.min.js';
  }

  const panel = document.getElementById('spotcheck-panel');
  const titleEl = document.getElementById('spotcheck-title');
  const contentEl = document.getElementById('spotcheck-content');
  const closeBtn = document.getElementById('spotcheck-close');

  // Workbench rendered without spotcheck infrastructure — bail out.
  if (!panel || !titleEl || !contentEl || !closeBtn) return;

  closeBtn.addEventListener('click', closeSpotcheck);

  // Close on Escape key when panel is open.
  document.addEventListener('keydown', function(e) {
    if (e.key === 'Escape' && !panel.classList.contains('hidden')) {
      closeSpotcheck();
    }
  });

  // Wire up every spotcheck-eligible field card. Rust emits a small
  // ↗ icon button at the top-right of cards whose evidence is a
  // document_span with filename + page + quote. Click opens the panel.
  document.querySelectorAll('.spotcheck-icon').forEach(function(btn) {
    btn.addEventListener('click', function(e) {
      e.preventDefault();
      e.stopPropagation();  // don't trigger the card's click-to-copy
      const filename = btn.dataset.filename;
      const page = parseInt(btn.dataset.page || '1', 10);
      const quote = btn.dataset.quote || '';
      if (!filename) return;
      const url = (window.SOURCE_DOCS_URL_PREFIX || 'files/') + filename;
      openSpotcheck(url, filename, page, quote);
    });
  });

  async function openSpotcheck(url, filename, page, quote) {
    panel.classList.remove('hidden');
    titleEl.textContent = 'Source: ' + filename + ' (page ' + page + ')';
    contentEl.innerHTML = '<p class="spotcheck-loading">Loading source document…</p>';

    const lower = url.toLowerCase();
    const isPdf = lower.endsWith('.pdf');
    try {
      if (isPdf) {
        await renderPdfPage(url, page);
      } else {
        renderImage(url);
      }
    } catch (err) {
      console.error('spotcheck: failed to load', url, err);
      contentEl.innerHTML =
        '<p class="spotcheck-error">Failed to load source document: ' +
        escapeHtml(String(err && err.message ? err.message : err)) +
        '</p>';
    }
  }

  function closeSpotcheck() {
    panel.classList.add('hidden');
    // Defer clearing the content so the slide-out animation looks
    // smooth (don't yank the canvas mid-transition).
    setTimeout(function() {
      if (panel.classList.contains('hidden')) {
        contentEl.innerHTML = '';
      }
    }, 250);
  }

  async function renderPdfPage(url, pageNum) {
    if (typeof window.pdfjsLib === 'undefined') {
      throw new Error('PDF.js failed to load (CDN unavailable?)');
    }
    contentEl.innerHTML = '<canvas id="spotcheck-canvas"></canvas>';
    const canvas = document.getElementById('spotcheck-canvas');
    const ctx = canvas.getContext('2d');

    const pdf = await window.pdfjsLib.getDocument(url).promise;
    const targetPage = Math.min(Math.max(1, pageNum), pdf.numPages);
    const page = await pdf.getPage(targetPage);
    const viewport = page.getViewport({ scale: 1.5 });

    canvas.width = viewport.width;
    canvas.height = viewport.height;

    await page.render({
      canvasContext: ctx,
      viewport: viewport,
    }).promise;
  }

  function renderImage(url) {
    const safe = escapeAttr(url);
    contentEl.innerHTML =
      '<img class="spotcheck-image" src="' + safe + '" alt="Source document">';
  }

  function escapeHtml(s) {
    return String(s)
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;');
  }

  function escapeAttr(s) {
    return String(s).replace(/"/g, '%22');
  }
})();
