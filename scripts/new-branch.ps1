<#
.SYNOPSIS
  Creates a new Maintafox feature, fix, hotfix, or chore branch from the correct base.
.DESCRIPTION
  Switches to the appropriate base branch (develop or main for hotfixes),
  pulls the latest changes, and creates a new branch following the project
  naming convention defined in docs/BRANCHING_STRATEGY.md.
.EXAMPLE
  .\scripts\new-branch.ps1 -Type feature -Slug p1-sp01-scaffold
  .\scripts\new-branch.ps1 -Type fix -Slug MAF-42-auth-offline-grace
  .\scripts\new-branch.ps1 -Type hotfix -Slug 1.0.1-critical-db-fix
  .\scripts\new-branch.ps1 -Type chore -Slug update-deps
#>
param(
    [Parameter(Mandatory=$true)]
    [ValidateSet("feature", "fix", "hotfix", "chore")]
    [string]$Type,

    [Parameter(Mandatory=$true)]
    [string]$Slug
)

# Validate slug is lowercase kebab-case
if ($Slug -notmatch '^[a-z0-9][a-z0-9\-]*[a-z0-9]$') {
    Write-Error "Slug must be lowercase kebab-case (e.g., p1-sp01-scaffold). Received: '$Slug'"
    exit 1
}

$BranchName = "$Type/$Slug"

if ($Type -eq "hotfix") {
    Write-Host "Switching to main and pulling latest..."
    git checkout main
    git pull origin main
} else {
    Write-Host "Switching to develop and pulling latest..."
    git checkout develop
    git pull origin develop
}

Write-Host "Creating branch: $BranchName"
git checkout -b $BranchName

Write-Host ""
Write-Host "Branch created: $BranchName" -ForegroundColor Green
Write-Host "When your work is ready, open a PR targeting:"
if ($Type -eq "hotfix") {
    Write-Host "  main (then a second PR to develop)" -ForegroundColor Yellow
} else {
    Write-Host "  develop" -ForegroundColor Yellow
}
