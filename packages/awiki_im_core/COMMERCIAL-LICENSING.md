# AWiki CLI S2 Dual-Licensing Policy

Version 1.0
Effective date: August 6, 2026
Software: AWiki CLI S2 (`awiki-cli-rs2`)
Licensor: **Hangzhou Vector Consensus Technology Co., Ltd.**
Commercial licensing contact: **chgaowei@gmail.com**

> This document explains the licensing policy. It is not itself an AWiki
> Commercial License and does not grant proprietary or closed-source rights.
> A user choosing the commercial path must register or apply and accept the
> applicable Commercial License Agreement, even when the license fee is zero.

## 1. Dual-licensing model

AWiki CLI S2 is offered under two alternative licensing paths:

1. GNU Affero General Public License, version 3 only (`AGPL-3.0-only`); or
2. a separate AWiki Commercial License.

Anyone who has not obtained an AWiki Commercial License may use the software
only under AGPLv3. A user may not combine selected permissions from one path
with selected permissions from the other to avoid either license's obligations.

## 2. AGPLv3 path

Any person or organization may use AWiki CLI S2 under AGPLv3 without a license
fee, regardless of revenue, commercial purpose, user count, or Agent Account
count. This path has no usage or Agent Account limit, permits commercial use,
modification, and distribution, and provides no exemption from AGPLv3.

Among other requirements, Section 13 of AGPLv3 requires an operator that
modifies the Program and lets users interact with it remotely through a
computer network to offer those users an opportunity to receive the
Corresponding Source. Distribution may trigger additional source-code,
notice, and downstream licensing requirements. The complete and controlling
terms are in [`LICENSE`](LICENSE).

The 100,000-Agent-Account threshold never limits AGPLv3 use. It determines
only whether the base fee for the commercial path is zero or paid.

## 3. AWiki Commercial License

A user that does not want to comply with the AGPLv3 obligations applicable to
its use must obtain an AWiki Commercial License before making that use.
Subject to its Commercial License Agreement, this path may permit private
modifications, closed-source integration and network services, proprietary
distribution, and internal or customer-facing commercial systems.

Commercial licenses are either Free Commercial Licenses or Paid Commercial
Licenses.

## 4. Free Commercial License

A licensee whose total number of enabled Agent Accounts does not exceed
100,000 may apply for a Free Commercial License. Its base license fee is zero.
Subject to the Commercial License Agreement, it may permit proprietary use,
modification, integration, deployment, and distribution, including OEM,
white-label, and customer-delivery use.

The licensee must register or apply, accept the Commercial License Agreement,
follow the counting rules, comply with intellectual-property and trademark
requirements, not circumvent license management, and transition under
Section 9 after exceeding the threshold. A license may be issued through
email confirmation, an electronic agreement, an order, or a written agreement.
An applicant should email the commercial licensing contact with its legal
name, contact person, intended use, and expected Agent Account count. Applying
does not itself grant a license; the license takes effect only when the
Licensor confirms the grant in writing or the parties accept or sign the
applicable commercial licensing document.

## 5. Paid Commercial License

Above 100,000 enabled Agent Accounts, a licensee must obtain a Paid Commercial
License to continue on the proprietary path. Pricing may use account counts or
tiers, annual subscription, perpetual license plus maintenance, enterprise
licensing, or another metric agreed in an order. The signed order, quote, or
contract controls pricing, payment, term, and capacity.

A user may instead move the relevant use entirely to AGPLv3, provided it can
and does comply fully with AGPLv3.

## 6. Agent Account definition

An **Agent Account** is an account or identity capable of acting as a distinct
logical agent through AWiki CLI S2 or a system built using it. An account
counts if it can do one or more of the following independently:

1. authenticate;
2. be addressed by a user, agent, or system;
3. send or receive messages;
4. maintain a mailbox, inbox, or message queue;
5. hold a key, credential, DID, Handle, or permission set;
6. invoke a service or perform tasks;
7. maintain business state, session state, permissions, or long-term memory;
   or
8. act for a distinct user, device, organization, or business principal.

Logical agents count separately even if they share a database account, API
key, connection, server, or technical account, if they can be addressed or
authorized independently, perform tasks independently, or maintain separate
state. An alias, display name, or routing address for the same agent does not
count again if it has no separate identity, permissions, or state.

## 7. Counting scope

The licensee must aggregate all enabled Agent Accounts operated, managed, or
served by the licensee and its Affiliates across all production environments,
tenants, regions, data centers, servers, clusters, products, business lines,
cloud and private deployments, accounts operated for customers, and accounts
customers create through the licensee's products.

The threshold may not be avoided by splitting companies, Affiliates, tenants,
servers, deployments, contracts, products, or technical accounts. Unless the
agreement states otherwise, the monthly count is the highest aggregate number
enabled at any time during that calendar month.

An **Affiliate** directly or indirectly controls, is controlled by, or is under
common control with the licensee. **Control** means ownership of more than 50
percent of voting interests or the power to direct management.

## 8. Development and test accounts

A development or test account is excluded only when it is used solely for
internal development, automated testing, or quality assurance; is reasonably
isolated from production; serves no real customer; processes no production
business; retains no long-lived production state; and is not used to avoid the
threshold. It counts as soon as it is used for a real user, customer,
production service, or production data.

## 9. Exceeding the free threshold

After first exceeding 100,000 enabled Agent Accounts, the licensee must notify
the Licensor within ten business days. It then has 30 calendar days to:

1. obtain a Paid Commercial License;
2. purchase AWiki Commercial Services that include the required license;
3. reduce the count to 100,000 or fewer;
4. stop proprietary use; or
5. move entirely to AGPLv3 and comply fully with it.

After that period, a licensee still above the threshold may not continue
proprietary use in reliance on the Free Commercial License.

## 10. Commercial Services

The Licensor may offer hosting, private deployment, support, service levels,
security maintenance, custom development, integration, operations, or upgrade
services. A commercial license may be included in the service fee; its scope
is controlled by the applicable order or services agreement.

A user at or below 100,000 Agent Accounts may apply for the base Free
Commercial License whether or not it buys services. Service fees pay primarily
for services and do not reduce rights already granted in an accepted license.

## 11. Distribution, OEM, and white-label use

Subject to an accepted Free Commercial License, a licensee at or below the
threshold may integrate AWiki CLI S2 into a proprietary product for OEM,
white-label, or customer delivery without a base license fee. Relevant Agent
Accounts operated, managed, or served by the licensee and its customers are
aggregated. Trademark, branding, certification, and joint-marketing rights
require separate written authorization.

## 12. Third-party components

Third-party components remain subject to their own licenses. An AWiki
Commercial License does not change them or grant rights the Licensor lacks.
Each user is responsible for complying with all applicable third-party terms.

## 13. External contributions

External contributions are not accepted unless the Licensor approves them in
writing. Before an approved contribution may be merged, the contributor must
sign or accept the contributor agreement designated by the Licensor, granting
sufficient open-source, proprietary licensing, patent, and sublicensing
rights. Accepted contributions may appear in AGPLv3 releases, free and paid
commercial releases, and AWiki Commercial Services.

## 14. Trademarks

No license automatically grants permission to use AWiki, AgentConnect, AWiki
CLI S2, or related trademarks, product names, domains, or logos. Truthful,
reasonable origin or compatibility statements are permitted, but users may
not imply official certification, sponsorship, or endorsement.

## 15. Examples

| Scenario | Available path |
| --- | --- |
| 50,000 Agent Accounts; AGPLv3 accepted | Free under AGPLv3; no commercial license required. |
| 50,000 Agent Accounts; proprietary use | An accepted Free Commercial License is required; base fee is zero. |
| 200,000 Agent Accounts; AGPLv3 accepted | Free under AGPLv3; no commercial license fee. |
| 200,000 Agent Accounts; proprietary use | A Paid Commercial License or services including it are required. |
| 50,000 Agent Accounts plus official hosting | The base license may be free; service fees still apply. |

> **AWiki CLI S2 is free without usage limits under AGPLv3. For proprietary or
> closed-source use, a commercial license is required, with no base license fee
> for deployments of up to 100,000 Agent Accounts.**

## 16. Documents and precedence

The paths are independent. For a commercial licensee, a formally signed
contract or order controls over the Commercial License Agreement to the extent
it expressly conflicts. For an AGPLv3 user, AGPLv3 controls. This policy is
explanatory and modifies neither license. README files, websites, and marketing
materials do not modify the applicable license or agreement.

## 17. Licensing transition

Releases through `cli-v1.0.41` were published under Apache License 2.0 and
remain under the rights already granted. AGPLv3 applies beginning with the
commit that introduces the AGPLv3 `LICENSE` and to later releases unless a
release expressly states otherwise. Existing Apache-licensed versions cannot
be retroactively withdrawn. The prior Apache License text is retained at
[`LICENSES/Apache-2.0.txt`](LICENSES/Apache-2.0.txt) for historical releases
and any portions for which its notices must be preserved.

## 18. Contact

Licensor: **Hangzhou Vector Consensus Technology Co., Ltd.**
Commercial licensing: **chgaowei@gmail.com**
