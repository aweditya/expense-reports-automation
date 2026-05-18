// Spotcheck side panel.
//
// When the FA clicks the ↗ icon on an extracted field card, this opens
// a side panel rendering the source receipt (PDF page or image) with an
// amber halo over the field's bbox(es) — computed at extract time by
// scripts/evidence_bbox.py via Google Document AI OCR, baked into
// _meta.evidence[].bboxes, and emitted as data-bboxes on the icon by
// the Rust render.
//
// Bboxes are normalized 0..1 with top-left origin. The overlay divs use
// percentage positioning relative to .spotcheck-page-container, which is
// inline-block and so matches the displayed canvas/image size — meaning
// the halos track correctly even if the canvas/image is shrunk by
// max-width.
//
// SOURCE_DOCS_URL_PREFIX is set by the Rust render. Defaults to "files/"
// (matches the Flask /uploads/<id>/files/ route on Cloud Run); override
// to "../../receipts/" for local CLI rendering or "/receipts/" when
// serving the local repo via `python -m http.server`.
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

  document.addEventListener('keydown', function(e) {
    if (e.key === 'Escape' && !panel.classList.contains('hidden')) {
      closeSpotcheck();
    }
  });

  document.querySelectorAll('.spotcheck-icon').forEach(function(btn) {
    btn.addEventListener('click', function(e) {
      e.preventDefault();
      e.stopPropagation();  // don't trigger the card's click-to-copy
      const filename = btn.dataset.filename;
      const page = parseInt(btn.dataset.page || '1', 10);
      const bboxes = parseBboxes(btn.dataset.bboxes);
      if (!filename) return;
      const url = (window.SOURCE_DOCS_URL_PREFIX || 'files/') + filename;
      openSpotcheck(url, filename, page, bboxes);
    });
  });

  // Parse the JSON-encoded data-bboxes attribute. Returns an array of
  // [x0, y0, x1, y1] rects (normalized 0..1, top-left), or [] if
  // unparseable or absent (cached extractions pre-Stage-B; quotes
  // Document AI couldn't match against OCR tokens).
  function parseBboxes(s) {
    if (!s) return [];
    try {
      const parsed = JSON.parse(s);
      if (!Array.isArray(parsed)) return [];
      return parsed.filter(function(r) {
        return Array.isArray(r) && r.length === 4 &&
          r.every(function(n) { return typeof n === 'number'; });
      });
    } catch (err) {
      return [];
    }
  }

  async function openSpotcheck(url, filename, page, bboxes) {
    panel.classList.remove('hidden');
    titleEl.textContent = 'Source: ' + filename + ' (page ' + page + ')';
    contentEl.innerHTML = '<p class="spotcheck-loading">Loading source document…</p>';

    const isPdf = url.toLowerCase().endsWith('.pdf');
    try {
      if (isPdf) {
        await renderPdfPage(url, page, bboxes);
      } else {
        await renderImage(url, bboxes);
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

  async function renderPdfPage(url, pageNum, bboxes) {
    if (typeof window.pdfjsLib === 'undefined') {
      throw new Error('PDF.js failed to load (CDN unavailable?)');
    }
    contentEl.innerHTML =
      '<div class="spotcheck-page-container">' +
        '<canvas id="spotcheck-canvas"></canvas>' +
      '</div>';
    const container = contentEl.querySelector('.spotcheck-page-container');
    const canvas = document.getElementById('spotcheck-canvas');
    const ctx = canvas.getContext('2d');

    const pdf = await window.pdfjsLib.getDocument(url).promise;
    const targetPage = Math.min(Math.max(1, pageNum), pdf.numPages);
    const page = await pdf.getPage(targetPage);
    const viewport = page.getViewport({ scale: 1.5 });

    canvas.width = viewport.width;
    canvas.height = viewport.height;

    await page.render({ canvasContext: ctx, viewport: viewport }).promise;
    drawBboxOverlays(container, bboxes);
  }

  async function renderImage(url, bboxes) {
    const safe = escapeAttr(url);
    contentEl.innerHTML =
      '<div class="spotcheck-page-container">' +
        '<img class="spotcheck-image" src="' + safe + '" alt="Source document">' +
      '</div>';
    const container = contentEl.querySelector('.spotcheck-page-container');
    const img = container.querySelector('img');

    // Wait until the image is loaded so the container has its final
    // displayed dimensions before we lay overlays over it.
    await new Promise(function(resolve, reject) {
      if (img.complete && img.naturalWidth > 0) {
        resolve();
      } else {
        img.onload = function() { resolve(); };
        img.onerror = function() {
          reject(new Error('image failed to load: ' + url));
        };
      }
    });
    drawBboxOverlays(container, bboxes);
  }

  // Draw amber overlay divs at each normalized bbox. Each rect is
  // [x0, y0, x1, y1] in 0..1 with top-left origin. Percentages on the
  // overlay divs mean they track the container's displayed size, so
  // they scale correctly when max-width shrinks the canvas/image.
  // Scrolls the first rect into view so the FA's eye lands on it.
  function drawBboxOverlays(container, bboxes) {
    if (!bboxes || bboxes.length === 0) return;
    let first = null;
    for (let i = 0; i < bboxes.length; i++) {
      const b = bboxes[i];
      const overlay = document.createElement('div');
      overlay.className = 'spotcheck-bbox-overlay';
      overlay.style.left = (b[0] * 100) + '%';
      overlay.style.top = (b[1] * 100) + '%';
      overlay.style.width = ((b[2] - b[0]) * 100) + '%';
      overlay.style.height = ((b[3] - b[1]) * 100) + '%';
      container.appendChild(overlay);
      if (i === 0) first = overlay;
    }
    if (first) {
      first.scrollIntoView({ behavior: 'smooth', block: 'center' });
    }
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
