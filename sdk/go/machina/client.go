// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Package machina is a small typed client for the Machina controller API.
//
// It covers the objects automation needs most (machines, hosts, tasks) and gives you a generic Do for the rest.
// Authenticate with an API key (Settings -> API keys): the key is sent as a bearer token.
package machina

import (
	"bytes"
	"context"
	"crypto/tls"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"
)

// Client talks to the controller (default port 5093), or to the daemon's same-origin proxy
// (https://host:5092/api/v1/platform/controller) when BaseURL points there.
type Client struct {
	BaseURL string
	Token   string
	HTTP    *http.Client
	// UserAgent is sent with every request.
	UserAgent string
}

// Option customises a Client.
type Option func(*Client)

// WithInsecureTLS accepts a self-signed controller certificate. Use only for lab setups.
func WithInsecureTLS() Option {
	return func(c *Client) {
		c.HTTP.Transport = &http.Transport{TLSClientConfig: &tls.Config{InsecureSkipVerify: true}} //nolint:gosec
	}
}

// WithHTTPClient replaces the underlying HTTP client.
func WithHTTPClient(h *http.Client) Option { return func(c *Client) { c.HTTP = h } }

// New returns a client for baseURL (for example "https://10.0.0.5:5093") authenticated with an API key.
func New(baseURL, token string, opts ...Option) *Client {
	c := &Client{
		BaseURL:   strings.TrimRight(baseURL, "/"),
		Token:     token,
		HTTP:      &http.Client{Timeout: 60 * time.Second},
		UserAgent: "machina-go-sdk/0.1",
	}
	for _, o := range opts {
		o(c)
	}
	return c
}

// APIError is a non-2xx answer from the controller.
type APIError struct {
	Status      int
	Message     string
	Code        string
	Remediation string
}

func (e *APIError) Error() string {
	s := fmt.Sprintf("machina: HTTP %d: %s", e.Status, e.Message)
	if e.Remediation != "" {
		s += " (" + e.Remediation + ")"
	}
	return s
}

// IsNotFound reports whether err is a 404 from the controller.
func IsNotFound(err error) bool {
	var ae *APIError
	return errors.As(err, &ae) && ae.Status == http.StatusNotFound
}

// Do performs a request and decodes a JSON response into out (which may be nil).
func (c *Client) Do(ctx context.Context, method, path string, query url.Values, body, out any) error {
	var rdr io.Reader
	if body != nil {
		b, err := json.Marshal(body)
		if err != nil {
			return err
		}
		rdr = bytes.NewReader(b)
	}
	u := c.BaseURL + path
	if len(query) > 0 {
		u += "?" + query.Encode()
	}
	req, err := http.NewRequestWithContext(ctx, method, u, rdr)
	if err != nil {
		return err
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("User-Agent", c.UserAgent)
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	if c.Token != "" {
		req.Header.Set("Authorization", "Bearer "+c.Token)
	}
	resp, err := c.HTTP.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	data, err := io.ReadAll(io.LimitReader(resp.Body, 32<<20))
	if err != nil {
		return err
	}
	if resp.StatusCode < 200 || resp.StatusCode > 299 {
		ae := &APIError{Status: resp.StatusCode, Message: strings.TrimSpace(string(data))}
		var parsed struct {
			Error       string `json:"error"`
			ErrorCode   string `json:"error_code"`
			Remediation string `json:"remediation"`
		}
		if json.Unmarshal(data, &parsed) == nil && parsed.Error != "" {
			ae.Message, ae.Code, ae.Remediation = parsed.Error, parsed.ErrorCode, parsed.Remediation
		}
		return ae
	}
	if out == nil || len(bytes.TrimSpace(data)) == 0 {
		return nil
	}
	return json.Unmarshal(data, out)
}
