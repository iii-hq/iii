# RFC Process

## Overview
The RFC (Request for Comments) process provides a consistent and collaborative path for proposing substantial changes to Motia. This ensures that major decisions are well-documented, thoroughly reviewed, and benefit from community input.

## When to Write an RFC
- New features that create public API surface area
- Breaking changes to existing functionality
- Significant architectural decisions
- New conventions, patterns, or development practices
- Changes that affect multiple teams or components

## Process Workflow
1. **Fork** this repository and create a new branch
2. **Copy** the template: `cp 0000-00-00-template.md text/YYYY-MM-DD-my-feature.md`
3. **Fill in** the RFC template with your proposal details
4. **Submit** a pull request with your RFC
5. **Iterate** based on community feedback and discussion
6. **Decision** by maintainers and implementation planning

## RFC States
- **Draft**: Under active discussion and iteration
- **Final Comment Period**: Last opportunity for feedback before decision
- **Accepted**: Approved for implementation with clear next steps
- **Rejected**: Not approved, with documented reasoning
- **Implemented**: Feature has been successfully built and deployed

## Review Criteria
RFCs are evaluated based on:
- Technical soundness and feasibility
- Alignment with Motia's vision and roadmap
- Impact on existing users and workflows
- Implementation complexity and maintenance burden
- Quality of documentation and examples