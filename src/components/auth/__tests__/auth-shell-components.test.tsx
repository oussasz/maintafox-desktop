import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { ActivationStepper } from "@/components/auth/activation/ActivationStepper";
import { ActivationTechnicalDetails } from "@/components/auth/activation/ActivationTechnicalDetails";
import {
  formatLicenseDate,
  OrganizationLicenseCard,
} from "@/components/auth/activation/OrganizationLicenseCard";
import { AuthField } from "@/components/auth/shell/AuthField";
import { AuthAlert, AuthButton } from "@/components/auth/shell/AuthPrimitives";

describe("AuthButton", () => {
  it("is disabled and marked busy while loading, so a second click cannot be sent", () => {
    render(<AuthButton loading>Valider</AuthButton>);
    const button = screen.getByRole("button", { name: "Valider" });
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute("aria-busy", "true");
  });

  it("is a plain button by default so it never submits a form by accident", () => {
    render(<AuthButton>Retour</AuthButton>);
    expect(screen.getByRole("button", { name: "Retour" })).toHaveAttribute("type", "button");
  });
});

describe("AuthAlert", () => {
  it("announces warnings and errors immediately, and information politely", () => {
    render(
      <>
        <AuthAlert tone="danger">Erreur</AuthAlert>
        <AuthAlert tone="warning">Attention</AuthAlert>
        <AuthAlert tone="info">Info</AuthAlert>
        <AuthAlert tone="success">OK</AuthAlert>
      </>,
    );
    expect(screen.getByText("Erreur").closest("[role]")).toHaveAttribute("role", "alert");
    expect(screen.getByText("Attention").closest("[role]")).toHaveAttribute("role", "alert");
    expect(screen.getByText("Info").closest("[role]")).toHaveAttribute("role", "status");
    expect(screen.getByText("OK").closest("[role]")).toHaveAttribute("role", "status");
  });
});

describe("AuthField", () => {
  it("links label, hint and error to the input for assistive technology", () => {
    render(<AuthField id="f" label="Identifiant" hint="Aide" error="Requis" />);
    const input = screen.getByLabelText("Identifiant");
    expect(input).toHaveAttribute("aria-invalid", "true");
    const describedBy = input.getAttribute("aria-describedby") ?? "";
    expect(describedBy.split(" ")).toHaveLength(2);
    expect(describedBy).toContain(screen.getByText("Requis").id);
    expect(describedBy).toContain(screen.getByText("Aide").id);
  });
});

describe("ActivationStepper", () => {
  const labels = { validate: "Valider", activate: "Activer", finish: "Terminer" };

  it("marks the current step for assistive technology and shows the compact progress text", () => {
    render(<ActivationStepper current="activate" labels={labels} progressLabel="Étape 2 sur 3" />);
    const steps = screen.getAllByRole("listitem");
    expect(steps).toHaveLength(3);
    expect(steps[1]).toHaveAttribute("aria-current", "step");
    expect(steps[0]).not.toHaveAttribute("aria-current");
    expect(screen.getByText("Étape 2 sur 3")).toBeInTheDocument();
  });
});

describe("formatLicenseDate", () => {
  it("formats an ISO timestamp for the UI language", () => {
    expect(formatLicenseDate("2027-09-10T07:42:00.000Z", "en")).toMatch(/2027/);
    expect(formatLicenseDate("2027-09-10T07:42:00.000Z", "fr")).toMatch(/2027/);
  });

  it("returns values that are not dates unchanged, so nothing the server sent is hidden", () => {
    expect(formatLicenseDate("never", "en")).toBe("never");
    expect(formatLicenseDate("—", "fr")).toBe("—");
  });
});

describe("OrganizationLicenseCard", () => {
  const labels = {
    companyBelongsTo: "Cette licence appartient à",
    slug: "Identifiant locataire",
    plan: "Offre",
    status: "Statut",
    channel: "Canal",
    expiration: "Expiration",
    allowedDevices: "Appareils autorisés",
    activatedDevices: "Appareils activés",
    deviceName: "Appareil actuel",
    notSpecified: "—",
  };

  it("shows the organisation, an active status chip and a readable expiry date", () => {
    render(
      <OrganizationLicenseCard
        labels={labels}
        fields={{
          companyName: "Demo Company",
          tenantSlug: "demo-1811",
          licensePlan: "development",
          licenseStatus: "active",
          channel: "stable",
          expiresAt: "2027-09-10T07:42:00.000Z",
          allowedDevices: 2,
          activatedDevices: 1,
          deviceName: "PC atelier",
        }}
      />,
    );
    expect(screen.getByRole("heading", { name: "Demo Company" })).toBeInTheDocument();
    expect(screen.getByText("active")).toBeInTheDocument();
    expect(screen.getByText(/2027/)).toBeInTheDocument();
    expect(screen.queryByText("2027-09-10T07:42:00.000Z")).not.toBeInTheDocument();
  });

  it("falls back to the placeholder for missing values", () => {
    render(<OrganizationLicenseCard labels={labels} fields={{ companyName: "X" }} />);
    expect(screen.getAllByText("—").length).toBeGreaterThan(3);
  });
});

describe("ActivationTechnicalDetails", () => {
  it("is collapsed by default and exposes its state", () => {
    render(
      <ActivationTechnicalDetails
        summaryLabel="Détails techniques"
        rows={[{ label: "Statut", value: "uninitialized" }]}
      />,
    );
    const toggle = screen.getByRole("button", { name: /Détails techniques/ });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("uninitialized")).not.toBeInTheDocument();
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("uninitialized")).toBeInTheDocument();
  });
});
