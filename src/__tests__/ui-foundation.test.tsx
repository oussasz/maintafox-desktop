// src/__tests__/ui-foundation.test.tsx
// Phase 2 · SP00-F04 · S1 — Integration tests verifying Shadcn components
// render correctly with the Maintafox Tailwind token system.

import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import {
  Badge,
  Button,
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
  DialogTrigger,
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
  Input,
  Label,
} from "@/components/ui";
import { Separator } from "@/components/ui/separator";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";

// ─── Button ───────────────────────────────────────────────────────

describe("Button", () => {
  it("renders with default variant", () => {
    render(<Button>Click me</Button>);
    const btn = screen.getByRole("button", { name: "Click me" });
    expect(btn).toBeInTheDocument();
    expect(btn.tagName).toBe("BUTTON");
  });

  it("renders with destructive variant", () => {
    render(<Button variant="destructive">Delete</Button>);
    const btn = screen.getByRole("button", { name: "Delete" });
    expect(btn).toBeInTheDocument();
    expect(btn.className).toContain("destructive");
  });

  it("renders with outline variant", () => {
    render(<Button variant="outline">Cancel</Button>);
    const btn = screen.getByRole("button", { name: "Cancel" });
    expect(btn).toBeInTheDocument();
    expect(btn.className).toContain("border");
  });

  it("supports disabled state", () => {
    render(<Button disabled>Disabled</Button>);
    expect(screen.getByRole("button", { name: "Disabled" })).toBeDisabled();
  });
});

// ─── Input ────────────────────────────────────────────────────────

describe("Input", () => {
  it("renders with placeholder text", () => {
    render(<Input placeholder="Enter value" />);
    expect(screen.getByPlaceholderText("Enter value")).toBeInTheDocument();
  });

  it("accepts user input", () => {
    render(<Input placeholder="Type here" />);
    const input = screen.getByPlaceholderText("Type here");
    fireEvent.change(input, { target: { value: "hello" } });
    expect(input).toHaveValue("hello");
  });
});

// ─── Label ────────────────────────────────────────────────────────

describe("Label", () => {
  it("renders with htmlFor attribute", () => {
    render(
      <>
        <Label htmlFor="test-input">Username</Label>
        <Input id="test-input" />
      </>,
    );
    const label = screen.getByText("Username");
    expect(label).toBeInTheDocument();
    expect(label).toHaveAttribute("for", "test-input");
  });
});

// ─── Dialog ───────────────────────────────────────────────────────

describe("Dialog", () => {
  it("opens when trigger is clicked and shows content", () => {
    render(
      <Dialog>
        <DialogTrigger asChild>
          <Button>Open Dialog</Button>
        </DialogTrigger>
        <DialogContent>
          <DialogTitle>Test Dialog</DialogTitle>
          <DialogDescription>Dialog body text</DialogDescription>
        </DialogContent>
      </Dialog>,
    );

    // Content should not be visible initially
    expect(screen.queryByText("Test Dialog")).not.toBeInTheDocument();

    // Click trigger
    fireEvent.click(screen.getByRole("button", { name: "Open Dialog" }));

    // Content should appear inside a dialog role
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByText("Test Dialog")).toBeInTheDocument();
    expect(screen.getByText("Dialog body text")).toBeInTheDocument();
  });

  it("closes when close button is clicked", () => {
    render(
      <Dialog>
        <DialogTrigger asChild>
          <Button>Open</Button>
        </DialogTrigger>
        <DialogContent>
          <DialogTitle>Closeable</DialogTitle>
          <DialogDescription>Can be closed</DialogDescription>
        </DialogContent>
      </Dialog>,
    );

    fireEvent.click(screen.getByRole("button", { name: "Open" }));
    expect(screen.getByRole("dialog")).toBeInTheDocument();

    // Click the Radix close button (sr-only "Close" text)
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});

// ─── DropdownMenu ─────────────────────────────────────────────────

describe("DropdownMenu", () => {
  it("renders trigger button with correct ARIA attributes", () => {
    render(
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button>Actions</Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent>
          <DropdownMenuItem>Edit</DropdownMenuItem>
          <DropdownMenuItem>Delete</DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>,
    );

    const trigger = screen.getByRole("button", { name: "Actions" });
    expect(trigger).toBeInTheDocument();
    expect(trigger).toHaveAttribute("aria-haspopup", "menu");
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    expect(trigger).toHaveAttribute("data-state", "closed");
  });
});

// ─── Tabs ─────────────────────────────────────────────────────────

describe("Tabs", () => {
  it("renders active content panel and hides inactive", () => {
    const { container } = render(
      <Tabs defaultValue="tab1">
        <TabsList>
          <TabsTrigger value="tab1">Tab One</TabsTrigger>
          <TabsTrigger value="tab2">Tab Two</TabsTrigger>
        </TabsList>
        <TabsContent value="tab1">Content One</TabsContent>
        <TabsContent value="tab2">Content Two</TabsContent>
      </Tabs>,
    );

    // Active panel content is rendered and visible
    expect(screen.getByText("Content One")).toBeVisible();

    // Inactive panel is in the DOM but hidden (Radix renders it with hidden attr)
    const allPanels = container.querySelectorAll('[role="tabpanel"]');
    expect(allPanels).toHaveLength(2);

    const inactivePanel = container.querySelector('[data-state="inactive"][role="tabpanel"]');
    expect(inactivePanel).not.toBeNull();
    expect(inactivePanel).toHaveAttribute("hidden");
  });

  it("preserves ARIA tab roles and selection state", () => {
    render(
      <Tabs defaultValue="a">
        <TabsList>
          <TabsTrigger value="a">Alpha</TabsTrigger>
          <TabsTrigger value="b">Beta</TabsTrigger>
        </TabsList>
        <TabsContent value="a">Alpha content</TabsContent>
        <TabsContent value="b">Beta content</TabsContent>
      </Tabs>,
    );

    const tablist = screen.getByRole("tablist");
    expect(tablist).toBeInTheDocument();

    const tabs = within(tablist).getAllByRole("tab");
    expect(tabs).toHaveLength(2);

    // Active tab should have aria-selected=true
    const alphaTab = screen.getByRole("tab", { name: "Alpha" });
    expect(alphaTab).toHaveAttribute("aria-selected", "true");
    expect(alphaTab).toHaveAttribute("data-state", "active");

    const betaTab = screen.getByRole("tab", { name: "Beta" });
    expect(betaTab).toHaveAttribute("aria-selected", "false");
    expect(betaTab).toHaveAttribute("data-state", "inactive");
  });
});

// ─── Badge ────────────────────────────────────────────────────────

describe("Badge", () => {
  it("renders with text", () => {
    render(<Badge>Active</Badge>);
    expect(screen.getByText("Active")).toBeInTheDocument();
  });

  it("renders with destructive variant", () => {
    render(<Badge variant="destructive">Critical</Badge>);
    const badge = screen.getByText("Critical");
    expect(badge).toBeInTheDocument();
    expect(badge.className).toContain("destructive");
  });

  it("renders with secondary variant", () => {
    render(<Badge variant="secondary">Pending</Badge>);
    const badge = screen.getByText("Pending");
    expect(badge).toBeInTheDocument();
    expect(badge.className).toContain("secondary");
  });
});

// ─── Card ─────────────────────────────────────────────────────────

describe("Card", () => {
  it("renders header and content", () => {
    render(
      <Card>
        <CardHeader>
          <CardTitle>Equipment Summary</CardTitle>
          <CardDescription>Overview of all assets</CardDescription>
        </CardHeader>
        <CardContent>
          <p>42 active assets</p>
        </CardContent>
      </Card>,
    );

    expect(screen.getByText("Equipment Summary")).toBeInTheDocument();
    expect(screen.getByText("Overview of all assets")).toBeInTheDocument();
    expect(screen.getByText("42 active assets")).toBeInTheDocument();
  });
});

// ─── Separator ────────────────────────────────────────────────────

describe("Separator", () => {
  it("renders with correct ARIA role", () => {
    render(<Separator data-testid="sep" />);
    // Radix Separator uses role="none" when decorative (default)
    const sep = screen.getByTestId("sep");
    expect(sep).toBeInTheDocument();
    expect(sep).toHaveAttribute("data-orientation", "horizontal");
  });
});
