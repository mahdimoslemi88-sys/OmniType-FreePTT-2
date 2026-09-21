# -*- coding: utf-8 -*-
"""
Executive DOCX Builder for Kynexa-AITCO
Standardized C-level Word document generator adhering to the corporate template:
- A4 size, 0.7in top/bottom, 0.75in left/right margins (6.80in printable width)
- Universal Tahoma typography (ascii, hAnsi, cs) with full RTL support
- Corporate palette: Deep Navy (#0F1E36), Sky Blue (#0284C7), Slate (#334155), Muted (#64748B)
- Professional tables with zebra striping and custom callout boxes
"""

import os
from pathlib import Path
from docx import Document
from docx.shared import Inches, Pt, RGBColor
from docx.enum.text import WD_ALIGN_PARAGRAPH
from docx.enum.table import WD_TABLE_ALIGNMENT
from docx.oxml import OxmlElement, parse_xml
from docx.oxml.ns import qn, nsdecls

# Corporate Executive Palette Constants
HEX_PRIMARY = "0F1E36"       # Deep Navy (Titles, Part Headings, Table Headers)
HEX_SECONDARY = "0284C7"     # Ocean Sky Blue (Chapter Headings, Subtitles, Callout Borders)
HEX_ACCENT = "38BDF8"        # Light Cyan (Key Highlights)
HEX_TEXT = "334155"          # Charcoal Slate (Body Text)
HEX_MUTED = "64748B"         # Muted Slate (Captions, Metadata)
HEX_BORDER = "CBD5E1"        # Light Gray (Table Inner Borders)
HEX_BG_LIGHT = "F8FAFC"      # Very Light Slate (Zebra Striping, Light Callout)
HEX_BG_INFO = "EFF6FF"       # Ice Blue (Info Callout Box)
HEX_WHITE = "FFFFFF"

CLR_PRIMARY = RGBColor(15, 30, 54)
CLR_SECONDARY = RGBColor(2, 132, 199)
CLR_ACCENT = RGBColor(56, 189, 248)
CLR_TEXT = RGBColor(51, 65, 85)
CLR_MUTED = RGBColor(100, 116, 139)
CLR_WHITE = RGBColor(255, 255, 255)

TOTAL_WIDTH_INCHES = 6.80

def set_p_rtl(p):
    """Enable RTL reading order on a paragraph."""
    pPr = p._p.get_or_add_pPr()
    bidi = pPr.find(qn('w:bidi'))
    if bidi is None:
        bidi = OxmlElement('w:bidi')
        pPr.append(bidi)
    bidi.set(qn('w:val'), '1')

def set_p_ltr(p):
    """Enable LTR reading order on a paragraph."""
    pPr = p._p.get_or_add_pPr()
    bidi = pPr.find(qn('w:bidi'))
    if bidi is None:
        bidi = OxmlElement('w:bidi')
        pPr.append(bidi)
    bidi.set(qn('w:val'), '0')

def set_run_font(run, font_name="Tahoma", size_pt=8.5, color_rgb=CLR_TEXT, bold=False, italic=False, is_rtl=True):
    """Apply consistent font attributes with complex script (cs) and RTL support."""
    run.font.name = font_name
    run.font.size = Pt(size_pt)
    run.font.color.rgb = color_rgb
    run.bold = bold
    run.italic = italic
    
    rPr = run._r.get_or_add_rPr()
    
    # Font bindings across ascii, hAnsi, and cs
    rFonts = rPr.find(qn('w:rFonts'))
    if rFonts is None:
        rFonts = OxmlElement('w:rFonts')
        rPr.append(rFonts)
    rFonts.set(qn('w:ascii'), font_name)
    rFonts.set(qn('w:hAnsi'), font_name)
    rFonts.set(qn('w:cs'), font_name)
    
    # RTL run property
    rtl = rPr.find(qn('w:rtl'))
    if rtl is None:
        rtl = OxmlElement('w:rtl')
        rPr.append(rtl)
    rtl.set(qn('w:val'), '1' if is_rtl else '0')

def set_table_rtl(table):
    """Make table display RTL."""
    tblPr = table._tbl.tblPr
    bidiVisual = tblPr.find(qn('w:bidiVisual'))
    if bidiVisual is None:
        bidiVisual = OxmlElement('w:bidiVisual')
        tblPr.append(bidiVisual)

def set_cell_shading(cell, color_hex):
    """Apply background color fill to a table cell."""
    tcPr = cell._tc.get_or_add_tcPr()
    shd = parse_xml(f'<w:shd {nsdecls("w")} w:fill="{color_hex}"/>')
    tcPr.append(shd)

def set_cell_margins(cell, top=50, bottom=50, left=60, right=60):
    """Set cell internal padding in dxa (1 pt = 20 dxa)."""
    tcPr = cell._tc.get_or_add_tcPr()
    tcMar = OxmlElement('w:tcMar')
    for m, val in [('top', top), ('bottom', bottom), ('left', left), ('right', right)]:
        node = OxmlElement(f'w:{m}')
        node.set(qn('w:w'), str(val))
        node.set(qn('w:type'), 'dxa')
        tcMar.append(node)
    tcPr.append(tcMar)

def set_cell_borders(cell, top=None, bottom=None, left=None, right=None):
    """
    Set cell borders individually.
    Format of border tuple: (val, sz, color_hex) e.g. ('single', '4', 'CBD5E1')
    """
    tcPr = cell._tc.get_or_add_tcPr()
    tcBorders = OxmlElement('w:tcBorders')
    for side, border in [('top', top), ('bottom', bottom), ('left', left), ('right', right)]:
        if border:
            val, sz, col = border
            node = OxmlElement(f'w:{side}')
            node.set(qn('w:val'), val)
            node.set(qn('w:sz'), str(sz))
            node.set(qn('w:space'), '0')
            node.set(qn('w:color'), col)
            tcBorders.append(node)
        else:
            node = OxmlElement(f'w:{side}')
            node.set(qn('w:val'), 'none')
            tcBorders.append(node)
    tcPr.append(tcBorders)

def create_executive_document():
    """Create an empty Document configured with standard A4 geometry and margins."""
    doc = Document()
    for section in doc.sections:
        # A4: 8.27in x 11.69in
        section.page_width = Inches(8.2701)
        section.page_height = Inches(11.6903)
        section.top_margin = Inches(0.70)
        section.bottom_margin = Inches(0.70)
        section.left_margin = Inches(0.75)
        section.right_margin = Inches(0.75)
        section.header_distance = Pt(36.0)
        section.footer_distance = Pt(36.0)
    return doc

def add_classification_banner(doc, text="طبقه‌بندی: محرمانه تجاری  |  اسناد راهبردی مهندسی و بازرگانی"):
    """Add the top classification navy ribbon."""
    tbl = doc.add_table(rows=1, cols=1)
    set_table_rtl(tbl)
    tbl.alignment = WD_TABLE_ALIGNMENT.CENTER
    cell = tbl.cell(0, 0)
    cell.width = Inches(TOTAL_WIDTH_INCHES)
    set_cell_shading(cell, HEX_PRIMARY)
    set_cell_margins(cell, top=90, bottom=90, left=150, right=150)
    
    p = cell.paragraphs[0]
    set_p_rtl(p)
    p.alignment = WD_ALIGN_PARAGRAPH.CENTER
    p.paragraph_format.space_before = Pt(0)
    p.paragraph_format.space_after = Pt(0)
    run = p.add_run(text)
    set_run_font(run, font_name="Tahoma", size_pt=9.5, color_rgb=CLR_WHITE, bold=True)
    
    # Spacer
    p_sp = doc.add_paragraph()
    p_sp.paragraph_format.space_before = Pt(0)
    p_sp.paragraph_format.space_after = Pt(4)

def add_cover_page(doc, title, subtitle_1="", subtitle_2="", scope_note="", metadata=None):
    """
    Generate the formal executive cover page with exact spacing, typography, and metadata box.
    """
    # Top spacing
    p_top = doc.add_paragraph()
    p_top.paragraph_format.space_before = Pt(30.0)
    p_top.paragraph_format.space_after = Pt(0)
    
    # Document Main Title
    p_title = doc.add_paragraph()
    set_p_rtl(p_title)
    p_title.alignment = WD_ALIGN_PARAGRAPH.CENTER
    p_title.paragraph_format.space_before = Pt(0)
    p_title.paragraph_format.space_after = Pt(14.0)
    p_title.paragraph_format.line_spacing = 1.35
    run_title = p_title.add_run(title)
    set_run_font(run_title, font_name="Tahoma", size_pt=23.0, color_rgb=CLR_PRIMARY, bold=True)
    
    # Subtitle 1
    if subtitle_1:
        p_sub1 = doc.add_paragraph()
        set_p_rtl(p_sub1)
        p_sub1.alignment = WD_ALIGN_PARAGRAPH.CENTER
        p_sub1.paragraph_format.space_before = Pt(0)
        p_sub1.paragraph_format.space_after = Pt(20.0)
        run_sub1 = p_sub1.add_run(subtitle_1)
        set_run_font(run_sub1, font_name="Tahoma", size_pt=14.0, color_rgb=CLR_SECONDARY, bold=True)
        
    # Subtitle 2
    if subtitle_2:
        p_sub2 = doc.add_paragraph()
        set_p_rtl(p_sub2)
        p_sub2.alignment = WD_ALIGN_PARAGRAPH.CENTER
        p_sub2.paragraph_format.space_before = Pt(0)
        p_sub2.paragraph_format.space_after = Pt(20.0)
        run_sub2 = p_sub2.add_run(subtitle_2)
        set_run_font(run_sub2, font_name="Tahoma", size_pt=10.5, color_rgb=CLR_SECONDARY, bold=True)
        
    # Scope note / Version
    if scope_note:
        p_scope = doc.add_paragraph()
        set_p_rtl(p_scope)
        p_scope.alignment = WD_ALIGN_PARAGRAPH.CENTER
        p_scope.paragraph_format.space_before = Pt(0)
        p_scope.paragraph_format.space_after = Pt(120.0)
        run_scope = p_scope.add_run(scope_note)
        set_run_font(run_scope, font_name="Tahoma", size_pt=9.5, color_rgb=CLR_MUTED, italic=True)
        
    # Metadata Table (4 columns)
    if metadata:
        meta_tbl = doc.add_table(rows=2, cols=len(metadata))
        set_table_rtl(meta_tbl)
        meta_tbl.alignment = WD_TABLE_ALIGNMENT.CENTER
        
        # Calculate col widths
        col_w = Inches(TOTAL_WIDTH_INCHES / len(metadata))
        
        # Row 0: Headers
        for idx, (lbl, _) in enumerate(metadata):
            cell = meta_tbl.cell(0, idx)
            cell.width = col_w
            set_cell_shading(cell, HEX_PRIMARY)
            set_cell_margins(cell, top=70, bottom=70, left=60, right=60)
            set_cell_borders(cell, 
                             top=('single', '6', HEX_PRIMARY),
                             bottom=('single', '6', HEX_PRIMARY),
                             left=('single', '6', HEX_PRIMARY),
                             right=('single', '6', HEX_PRIMARY))
            p = cell.paragraphs[0]
            set_p_rtl(p)
            p.alignment = WD_ALIGN_PARAGRAPH.CENTER
            p.paragraph_format.space_before = Pt(0)
            p.paragraph_format.space_after = Pt(0)
            r = p.add_run(lbl)
            set_run_font(r, font_name="Tahoma", size_pt=9.5, color_rgb=CLR_WHITE, bold=True)
            
        # Row 1: Values
        for idx, (_, val) in enumerate(metadata):
            cell = meta_tbl.cell(1, idx)
            cell.width = col_w
            set_cell_shading(cell, HEX_WHITE)
            set_cell_margins(cell, top=50, bottom=50, left=60, right=60)
            set_cell_borders(cell, 
                             top=('single', '4', HEX_BORDER),
                             bottom=('single', '4', HEX_BORDER),
                             left=('single', '4', HEX_BORDER),
                             right=('single', '4', HEX_BORDER))
            p = cell.paragraphs[0]
            set_p_rtl(p)
            p.alignment = WD_ALIGN_PARAGRAPH.CENTER
            p.paragraph_format.space_before = Pt(0)
            p.paragraph_format.space_after = Pt(0)
            r = p.add_run(val)
            set_run_font(r, font_name="Tahoma", size_pt=9.0, color_rgb=CLR_TEXT)
            
    # Page break after cover
    doc.add_page_break()

def add_part_heading(doc, text):
    """Add a major Part heading (بخش X) - 20pt Bold Navy."""
    p = doc.add_paragraph()
    set_p_rtl(p)
    p.alignment = WD_ALIGN_PARAGRAPH.RIGHT
    p.paragraph_format.space_before = Pt(14.0)
    p.paragraph_format.space_after = Pt(4.0)
    run = p.add_run(text)
    set_run_font(run, font_name="Tahoma", size_pt=20.0, color_rgb=CLR_PRIMARY, bold=True)
    return p

def add_chapter_heading(doc, text):
    """Add a Chapter heading (فصل X) - 14pt Bold Sky Blue."""
    p = doc.add_paragraph()
    set_p_rtl(p)
    p.alignment = WD_ALIGN_PARAGRAPH.RIGHT
    p.paragraph_format.space_before = Pt(10.0)
    p.paragraph_format.space_after = Pt(3.0)
    run = p.add_run(text)
    set_run_font(run, font_name="Tahoma", size_pt=14.0, color_rgb=CLR_SECONDARY, bold=True)
    return p

def add_subheading(doc, text):
    """Add a Subheading / Subsection - 10.5pt Bold Sky Blue."""
    p = doc.add_paragraph()
    set_p_rtl(p)
    p.alignment = WD_ALIGN_PARAGRAPH.RIGHT
    p.paragraph_format.space_before = Pt(8.0)
    p.paragraph_format.space_after = Pt(3.0)
    run = p.add_run(text)
    set_run_font(run, font_name="Tahoma", size_pt=10.5, color_rgb=CLR_SECONDARY, bold=True)
    return p

def add_body_paragraph(doc, text="", bold_prefix="", space_after=4.0, line_spacing=1.18, align=WD_ALIGN_PARAGRAPH.JUSTIFY):
    """
    Add a standard justified body paragraph with optional bold lead-in prefix.
    """
    p = doc.add_paragraph()
    set_p_rtl(p)
    p.alignment = align
    p.paragraph_format.space_before = Pt(0)
    p.paragraph_format.space_after = Pt(space_after)
    p.paragraph_format.line_spacing = line_spacing
    
    if bold_prefix:
        r_pre = p.add_run(bold_prefix + " ")
        set_run_font(r_pre, font_name="Tahoma", size_pt=9.0, color_rgb=CLR_PRIMARY, bold=True)
        
    if text:
        r_txt = p.add_run(text)
        set_run_font(r_txt, font_name="Tahoma", size_pt=8.5, color_rgb=CLR_TEXT)
        
    return p

def add_bullet_item(doc, text="", bold_prefix="", bullet_char="▪", space_after=3.0):
    """Add a customized bullet point with Persian bullet character."""
    p = doc.add_paragraph()
    set_p_rtl(p)
    p.alignment = WD_ALIGN_PARAGRAPH.JUSTIFY
    p.paragraph_format.space_before = Pt(0)
    p.paragraph_format.space_after = Pt(space_after)
    p.paragraph_format.line_spacing = 1.18
    
    prefix = f"{bullet_char} {bold_prefix} " if bold_prefix else f"{bullet_char} "
    r_pre = p.add_run(prefix)
    set_run_font(r_pre, font_name="Tahoma", size_pt=8.5, color_rgb=CLR_PRIMARY, bold=True)
    
    if text:
        r_txt = p.add_run(text)
        set_run_font(r_txt, font_name="Tahoma", size_pt=8.5, color_rgb=CLR_TEXT)
        
    return p

def add_callout_box(doc, text, title="", box_type="info", width_in=TOTAL_WIDTH_INCHES):
    """
    Add a styled executive callout box (Info Blue, Light Slate, or Deep Navy).
    """
    tbl = doc.add_table(rows=1, cols=1)
    set_table_rtl(tbl)
    tbl.alignment = WD_TABLE_ALIGNMENT.CENTER
    cell = tbl.cell(0, 0)
    cell.width = Inches(width_in)
    
    if box_type == "info":
        set_cell_shading(cell, HEX_BG_INFO)
        set_cell_borders(cell, 
                         top=('single', '12', HEX_SECONDARY),
                         bottom=('single', '12', HEX_SECONDARY),
                         left=('single', '12', HEX_SECONDARY),
                         right=('single', '12', HEX_SECONDARY))
        title_color = CLR_PRIMARY
        text_color = CLR_TEXT
    elif box_type == "navy":
        set_cell_shading(cell, HEX_PRIMARY)
        set_cell_borders(cell, 
                         top=('single', '12', HEX_SECONDARY),
                         bottom=('single', '12', HEX_SECONDARY),
                         left=('single', '12', HEX_SECONDARY),
                         right=('single', '12', HEX_SECONDARY))
        title_color = CLR_WHITE
        text_color = CLR_WHITE
    else:  # note / gray
        set_cell_shading(cell, HEX_BG_LIGHT)
        set_cell_borders(cell, 
                         top=('single', '12', HEX_SECONDARY),
                         bottom=('single', '12', HEX_SECONDARY),
                         left=('single', '12', HEX_SECONDARY),
                         right=('single', '12', HEX_SECONDARY))
        title_color = CLR_PRIMARY
        text_color = CLR_TEXT
        
    set_cell_margins(cell, top=80, bottom=80, left=140, right=160)
    
    p = cell.paragraphs[0]
    set_p_rtl(p)
    p.alignment = WD_ALIGN_PARAGRAPH.JUSTIFY
    p.paragraph_format.space_before = Pt(0)
    p.paragraph_format.space_after = Pt(0)
    p.paragraph_format.line_spacing = 1.18
    
    if title:
        r_title = p.add_run(title + "\n")
        set_run_font(r_title, font_name="Tahoma", size_pt=9.5, color_rgb=title_color, bold=True)
        
    r_body = p.add_run(text)
    set_run_font(r_body, font_name="Tahoma", size_pt=8.5, color_rgb=text_color)
    
    p_sp = doc.add_paragraph()
    p_sp.paragraph_format.space_before = Pt(0)
    p_sp.paragraph_format.space_after = Pt(4)

def add_styled_table(doc, headers, rows_data, col_widths=None, align_center_cols=None):
    """
    Generate an executive table with deep navy headers and alternating zebra striping.
    """
    tbl = doc.add_table(rows=len(rows_data) + 1, cols=len(headers))
    set_table_rtl(tbl)
    tbl.alignment = WD_TABLE_ALIGNMENT.CENTER
    
    # Calculate widths
    if not col_widths:
        w_per_col = TOTAL_WIDTH_INCHES / len(headers)
        col_widths = [Inches(w_per_col)] * len(headers)
    else:
        col_widths = [Inches(w) if isinstance(w, (int, float)) else w for w in col_widths]
        
    align_center_cols = align_center_cols or []
    
    # Header Row
    for c_idx, h_text in enumerate(headers):
        cell = tbl.cell(0, c_idx)
        cell.width = col_widths[c_idx]
        set_cell_shading(cell, HEX_PRIMARY)
        set_cell_margins(cell, top=70, bottom=70, left=60, right=60)
        set_cell_borders(cell, 
                         top=('single', '6', HEX_PRIMARY),
                         bottom=('single', '6', HEX_PRIMARY),
                         left=('single', '6', HEX_PRIMARY),
                         right=('single', '6', HEX_PRIMARY))
        p = cell.paragraphs[0]
        set_p_rtl(p)
        p.alignment = WD_ALIGN_PARAGRAPH.CENTER
        p.paragraph_format.space_before = Pt(0)
        p.paragraph_format.space_after = Pt(0)
        r = p.add_run(h_text)
        set_run_font(r, font_name="Tahoma", size_pt=9.0, color_rgb=CLR_WHITE, bold=True)
        
    # Data Rows
    for r_idx, row in enumerate(rows_data, 1):
        bg_col = HEX_WHITE if r_idx % 2 == 1 else HEX_BG_LIGHT
        for c_idx, val in enumerate(row):
            cell = tbl.cell(r_idx, c_idx)
            cell.width = col_widths[c_idx]
            set_cell_shading(cell, bg_col)
            set_cell_margins(cell, top=50, bottom=50, left=60, right=60)
            set_cell_borders(cell, 
                             top=('single', '4', HEX_BORDER),
                             bottom=('single', '4', HEX_BORDER),
                             left=('single', '4', HEX_BORDER),
                             right=('single', '4', HEX_BORDER))
            p = cell.paragraphs[0]
            set_p_rtl(p)
            p.alignment = WD_ALIGN_PARAGRAPH.CENTER if c_idx in align_center_cols else WD_ALIGN_PARAGRAPH.RIGHT
            p.paragraph_format.space_before = Pt(0)
            p.paragraph_format.space_after = Pt(0)
            p.paragraph_format.line_spacing = 1.15
            r = p.add_run(str(val))
            # First column or key column can be bold navy
            is_bold = (c_idx == 0)
            text_color = CLR_PRIMARY if is_bold else CLR_TEXT
            set_run_font(r, font_name="Tahoma", size_pt=8.5, color_rgb=text_color, bold=is_bold)
            
    p_sp = doc.add_paragraph()
    p_sp.paragraph_format.space_before = Pt(0)
    p_sp.paragraph_format.space_after = Pt(6)

def add_image_with_caption(doc, image_path, caption="", width_in=5.40):
    """Add a centered image with standard 7.5pt muted italic caption."""
    p_img = doc.add_paragraph()
    p_img.alignment = WD_ALIGN_PARAGRAPH.CENTER
    p_img.paragraph_format.space_before = Pt(4.0)
    p_img.paragraph_format.space_after = Pt(2.0)
    
    run_img = p_img.add_run()
    run_img.add_picture(str(image_path), width=Inches(width_in))
    
    if caption:
        p_cap = doc.add_paragraph()
        set_p_rtl(p_cap)
        p_cap.alignment = WD_ALIGN_PARAGRAPH.CENTER
        p_cap.paragraph_format.space_before = Pt(0)
        p_cap.paragraph_format.space_after = Pt(6.0)
        run_cap = p_cap.add_run(caption)
        set_run_font(run_cap, font_name="Tahoma", size_pt=7.5, color_rgb=CLR_MUTED, italic=True)

def save_document(doc, output_path):
    """Ensure directory exists and save document."""
    out_file = Path(output_path)
    out_file.parent.mkdir(parents=True, exist_ok=True)
    doc.save(out_file)
    print(f"[+] Successfully saved executive DOCX: {out_file.resolve()}")
