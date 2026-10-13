import Image from "@tiptap/extension-image";
import Placeholder from "@tiptap/extension-placeholder";
import TextAlign from "@tiptap/extension-text-align";
import Underline from "@tiptap/extension-underline";
import { EditorContent, useEditor, type JSONContent } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import {
  AlignCenter,
  AlignLeft,
  AlignRight,
  Bold,
  Heading2,
  ImageIcon,
  Italic,
  List,
  ListOrdered,
  Redo2,
  Underline as UnderlineIcon,
  Undo2,
} from "lucide-react";
import { useEffect } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

export interface DocumentHeaderEditorProps {
  contentJson: unknown;
  onChange: (payload: { contentJson: JSONContent; html: string }) => void;
  disabled?: boolean;
  className?: string;
  /** Insert a data-URL image (logo) into the letterhead. */
  insertImageSrc?: string | null;
  onInsertImageConsumed?: () => void;
}

function ToolbarButton({
  label,
  active,
  disabled,
  onClick,
  children,
}: {
  label: string;
  active?: boolean;
  disabled?: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <Button
      type="button"
      variant={active ? "secondary" : "ghost"}
      size="sm"
      className="h-8 w-8 p-0"
      disabled={disabled}
      aria-label={label}
      title={label}
      onClick={onClick}
    >
      {children}
    </Button>
  );
}

export function DocumentHeaderEditor({
  contentJson,
  onChange,
  disabled = false,
  className,
  insertImageSrc,
  onInsertImageConsumed,
}: DocumentHeaderEditorProps) {
  const { t } = useTranslation("settings");

  const editor = useEditor({
    extensions: [
      StarterKit.configure({
        heading: { levels: [1, 2, 3] },
      }),
      Underline,
      TextAlign.configure({
        types: ["heading", "paragraph"],
      }),
      Image.configure({
        inline: false,
        allowBase64: true,
      }),
      Placeholder.configure({
        placeholder: t("documentHeader.editorLabel"),
      }),
    ],
    content: (contentJson as JSONContent) ?? { type: "doc", content: [{ type: "paragraph" }] },
    editable: !disabled,
    editorProps: {
      attributes: {
        class:
          "prose prose-sm max-w-none min-h-[9rem] px-3 py-2 focus:outline-none text-text-primary",
      },
    },
    onUpdate: ({ editor: ed }) => {
      onChange({
        contentJson: ed.getJSON(),
        html: ed.getHTML(),
      });
    },
  });

  useEffect(() => {
    if (!editor) return;
    editor.setEditable(!disabled);
  }, [disabled, editor]);

  useEffect(() => {
    if (!editor || !insertImageSrc) return;
    editor
      .chain()
      .focus()
      .setImage({ src: insertImageSrc, alt: t("documentHeader.logoLabel") })
      .run();
    onInsertImageConsumed?.();
  }, [editor, insertImageSrc, onInsertImageConsumed, t]);

  if (!editor) return null;

  return (
    <div className={cn("rounded-md border border-surface-border bg-surface-1", className)}>
      <div className="flex flex-wrap items-center gap-0.5 border-b border-surface-border px-2 py-1.5">
        <ToolbarButton
          label={t("documentHeader.toolbar.bold")}
          active={editor.isActive("bold")}
          disabled={disabled}
          onClick={() => editor.chain().focus().toggleBold().run()}
        >
          <Bold className="h-4 w-4" />
        </ToolbarButton>
        <ToolbarButton
          label={t("documentHeader.toolbar.italic")}
          active={editor.isActive("italic")}
          disabled={disabled}
          onClick={() => editor.chain().focus().toggleItalic().run()}
        >
          <Italic className="h-4 w-4" />
        </ToolbarButton>
        <ToolbarButton
          label={t("documentHeader.toolbar.underline")}
          active={editor.isActive("underline")}
          disabled={disabled}
          onClick={() => editor.chain().focus().toggleUnderline().run()}
        >
          <UnderlineIcon className="h-4 w-4" />
        </ToolbarButton>
        <ToolbarButton
          label={t("documentHeader.toolbar.heading")}
          active={editor.isActive("heading", { level: 2 })}
          disabled={disabled}
          onClick={() => editor.chain().focus().toggleHeading({ level: 2 }).run()}
        >
          <Heading2 className="h-4 w-4" />
        </ToolbarButton>
        <ToolbarButton
          label={t("documentHeader.toolbar.bulletList")}
          active={editor.isActive("bulletList")}
          disabled={disabled}
          onClick={() => editor.chain().focus().toggleBulletList().run()}
        >
          <List className="h-4 w-4" />
        </ToolbarButton>
        <ToolbarButton
          label={t("documentHeader.toolbar.orderedList")}
          active={editor.isActive("orderedList")}
          disabled={disabled}
          onClick={() => editor.chain().focus().toggleOrderedList().run()}
        >
          <ListOrdered className="h-4 w-4" />
        </ToolbarButton>
        <ToolbarButton
          label={t("documentHeader.toolbar.alignLeft")}
          active={editor.isActive({ textAlign: "left" })}
          disabled={disabled}
          onClick={() => editor.chain().focus().setTextAlign("left").run()}
        >
          <AlignLeft className="h-4 w-4" />
        </ToolbarButton>
        <ToolbarButton
          label={t("documentHeader.toolbar.alignCenter")}
          active={editor.isActive({ textAlign: "center" })}
          disabled={disabled}
          onClick={() => editor.chain().focus().setTextAlign("center").run()}
        >
          <AlignCenter className="h-4 w-4" />
        </ToolbarButton>
        <ToolbarButton
          label={t("documentHeader.toolbar.alignRight")}
          active={editor.isActive({ textAlign: "right" })}
          disabled={disabled}
          onClick={() => editor.chain().focus().setTextAlign("right").run()}
        >
          <AlignRight className="h-4 w-4" />
        </ToolbarButton>
        <ToolbarButton
          label={t("documentHeader.toolbar.undo")}
          disabled={disabled || !editor.can().undo()}
          onClick={() => editor.chain().focus().undo().run()}
        >
          <Undo2 className="h-4 w-4" />
        </ToolbarButton>
        <ToolbarButton
          label={t("documentHeader.toolbar.redo")}
          disabled={disabled || !editor.can().redo()}
          onClick={() => editor.chain().focus().redo().run()}
        >
          <Redo2 className="h-4 w-4" />
        </ToolbarButton>
        <span className="mx-1 text-text-muted" aria-hidden>
          |
        </span>
        <span className="inline-flex items-center gap-1 px-1 text-xs text-text-muted">
          <ImageIcon className="h-3.5 w-3.5" aria-hidden />
          {t("documentHeader.toolbar.insertImage")}
        </span>
      </div>
      <EditorContent editor={editor} />
    </div>
  );
}
